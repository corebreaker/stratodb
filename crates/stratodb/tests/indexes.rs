//! Secondary-index registry: creation, idempotency, divergence detection,
//! per-table scoping and persistence — through the public `Table` API.

use stratodb::{
    data::{MapMut, Scalar},
    index::{IndexColumn, IndexDef},
    path::SPath,
    SdbError,
    StratoDb,
};

use std::collections::BTreeMap;

/// A single-column ascending index over `users/*` at `column`.
fn single(name: &str, column: &str, unique: bool) -> IndexDef {
    IndexDef::new(
        name.to_string(),
        String::from("users/*"),
        vec![IndexColumn::asc(SPath::parse(column).unwrap())],
        unique,
    )
}

fn def(name: &str, unique: bool) -> IndexDef {
    IndexDef::new(
        name.to_string(),
        String::from("users/*"),
        vec![
            IndexColumn::asc(SPath::parse("age").unwrap()),
            IndexColumn::desc(SPath::parse("name").unwrap()),
        ],
        unique,
    )
}

#[test]
fn create_index_registers_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("idx.stratodb")).unwrap();
    let users = db.open_table("users").unwrap();

    users.create_index(&def("by_age_name", false)).unwrap();
    users.create_index(&def("by_age_name", false)).unwrap(); // identical -> idempotent

    assert_eq!(users.index_def("by_age_name").unwrap(), Some(def("by_age_name", false)));
    assert!(users.index_def("missing").unwrap().is_none());
}

#[test]
fn create_index_rejects_a_divergent_redefinition() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("idx_diverge.stratodb")).unwrap();
    let users = db.open_table("users").unwrap();

    users.create_index(&def("by_age_name", false)).unwrap();

    // Same name, different definition (unique flag flipped) -> error.
    let err = users.create_index(&def("by_age_name", true)).unwrap_err();
    assert!(
        matches!(err, SdbError::SchemaMismatch(_)),
        "expected SchemaMismatch, got {err:?}"
    );
}

#[test]
fn indexes_are_scoped_per_table() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("idx_scope.stratodb")).unwrap();
    let users = db.open_table("users").unwrap();
    let posts = db.open_table("posts").unwrap();

    users.create_index(&def("by_x", false)).unwrap();

    assert!(users.index_def("by_x").unwrap().is_some());
    // Same index name on another table is independent.
    assert!(posts.index_def("by_x").unwrap().is_none());
}

#[test]
fn index_definitions_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("idx_reopen.stratodb");

    {
        let db = StratoDb::create(&path).unwrap();
        db.open_table("users")
            .unwrap()
            .create_index(&def("by_age_name", true))
            .unwrap();
    }

    let db = StratoDb::open(&path).unwrap();
    assert_eq!(
        db.open_table("users").unwrap().index_def("by_age_name").unwrap(),
        Some(def("by_age_name", true))
    );
}

// --------------------------------------------------------------------------
// End-to-end maintenance and exact-match queries
// --------------------------------------------------------------------------

/// How many entities the index reports for the exact column value `age`.
fn count_at(db: &StratoDb, table: &str, index: &str, age: i32) -> usize {
    db.open_table(table)
        .unwrap()
        .read()
        .unwrap()
        .find::<BTreeMap<String, i32>>(index, &[Scalar::I32(age)])
        .unwrap()
        .len()
}

#[test]
fn index_tracks_store_update_and_remove() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("e2e.stratodb")).unwrap();
    let users = db.open_table("users").unwrap();
    users.create_index(&single("by_age", "age", false)).unwrap();

    let w = users.write().unwrap();
    w.put("users/alice/age", &30i32).unwrap();
    w.put("users/bob/age", &30i32).unwrap();
    w.put("users/carol/age", &40i32).unwrap();
    w.commit().unwrap();

    // The matching entity is recomposed from its own subtree.
    let found = users
        .read()
        .unwrap()
        .find::<BTreeMap<String, i32>>("by_age", &[Scalar::I32(40)])
        .unwrap();
    assert_eq!(found, vec![BTreeMap::from([(String::from("age"), 40)])]);
    assert_eq!(count_at(&db, "users", "by_age", 30), 2);

    // Updating a column moves the entity between buckets.
    let w = users.write().unwrap();
    w.put("users/alice/age", &40i32).unwrap();
    w.commit().unwrap();
    assert_eq!(count_at(&db, "users", "by_age", 30), 1);
    assert_eq!(count_at(&db, "users", "by_age", 40), 2);

    // Removing an entity drops its entry.
    let w = users.write().unwrap();
    assert!(w.remove("users/bob").unwrap());
    w.commit().unwrap();
    assert_eq!(count_at(&db, "users", "by_age", 30), 0);
    assert_eq!(count_at(&db, "users", "by_age", 40), 2);
}

#[test]
fn find_rejects_unknown_index_and_wrong_arity() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("e2e_errors.stratodb")).unwrap();
    let users = db.open_table("users").unwrap();
    users.create_index(&single("by_age", "age", false)).unwrap();

    let r = users.read().unwrap();

    let missing = r.find::<BTreeMap<String, i32>>("nope", &[Scalar::I32(1)]).unwrap_err();
    assert!(matches!(missing, SdbError::IndexNotFound { .. }), "got {missing:?}");

    let arity = r
        .find::<BTreeMap<String, i32>>("by_age", &[Scalar::I32(1), Scalar::I32(2)])
        .unwrap_err();
    assert!(matches!(arity, SdbError::IndexArity { .. }), "got {arity:?}");
}

#[test]
fn storing_a_whole_subtree_indexes_every_child() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("e2e_subtree.stratodb")).unwrap();
    let users = db.open_table("users").unwrap();
    users.create_index(&single("by_age", "age", false)).unwrap();

    // A single `store` above the matched entities must index each child.
    let everyone: BTreeMap<String, BTreeMap<String, i32>> = BTreeMap::from([
        (String::from("alice"), BTreeMap::from([(String::from("age"), 30)])),
        (String::from("bob"), BTreeMap::from([(String::from("age"), 30)])),
    ]);

    let w = users.write().unwrap();
    w.store("users", &everyone).unwrap();
    w.commit().unwrap();
    assert_eq!(count_at(&db, "users", "by_age", 30), 2);

    // Clearing the container (above the entities) drops every entry.
    let w = users.write().unwrap();
    w.fetch_mut::<MapMut<BTreeMap<String, i32>>>("users")
        .unwrap()
        .clear()
        .unwrap();
    w.commit().unwrap();
    assert_eq!(count_at(&db, "users", "by_age", 30), 0);
}

#[test]
fn composite_index_matches_on_every_column() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("e2e_composite.stratodb")).unwrap();
    let users = db.open_table("users").unwrap();

    // Two columns, the second descending, to exercise multi-column encoding.
    let idx = IndexDef::new(
        String::from("by_a_b"),
        String::from("users/*"),
        vec![
            IndexColumn::asc(SPath::parse("a").unwrap()),
            IndexColumn::desc(SPath::parse("b").unwrap()),
        ],
        false,
    );
    users.create_index(&idx).unwrap();

    let w = users.write().unwrap();
    for (name, a, b) in [("x", 1, 5), ("y", 1, 9), ("z", 2, 5)] {
        w.put(&format!("users/{name}/a"), &a).unwrap();
        w.put(&format!("users/{name}/b"), &b).unwrap();
    }
    w.commit().unwrap();

    let r = users.read().unwrap();

    // The full tuple is needed; only the exact (a, b) matches.
    let xs = r
        .find::<BTreeMap<String, i32>>("by_a_b", &[Scalar::I32(1), Scalar::I32(5)])
        .unwrap();
    assert_eq!(
        xs,
        vec![BTreeMap::from([(String::from("a"), 1), (String::from("b"), 5)])]
    );
    assert_eq!(
        r.find::<BTreeMap<String, i32>>("by_a_b", &[Scalar::I32(1), Scalar::I32(9)])
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        r.find::<BTreeMap<String, i32>>("by_a_b", &[Scalar::I32(2), Scalar::I32(9)])
            .unwrap()
            .len(),
        0
    );

    // A partial key is rejected — exact match needs every column.
    let arity = r
        .find::<BTreeMap<String, i32>>("by_a_b", &[Scalar::I32(1)])
        .unwrap_err();
    assert!(matches!(arity, SdbError::IndexArity { .. }), "got {arity:?}");
}

#[test]
fn unique_index_keeps_the_entity_in_the_value() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("e2e_unique.stratodb")).unwrap();
    let users = db.open_table("users").unwrap();
    users.create_index(&single("uby_age", "age", true)).unwrap();

    let w = users.write().unwrap();
    w.put("users/alice/age", &30i32).unwrap();
    w.put("users/bob/age", &40i32).unwrap();
    w.commit().unwrap();

    let r = users.read().unwrap();
    assert_eq!(
        r.find::<BTreeMap<String, i32>>("uby_age", &[Scalar::I32(30)]).unwrap(),
        vec![BTreeMap::from([(String::from("age"), 30)])]
    );
    assert_eq!(
        r.find::<BTreeMap<String, i32>>("uby_age", &[Scalar::I32(40)])
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        r.find::<BTreeMap<String, i32>>("uby_age", &[Scalar::I32(99)])
            .unwrap()
            .len(),
        0
    );
}
