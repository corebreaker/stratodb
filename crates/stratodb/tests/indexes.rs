//! Secondary-index registry: creation, idempotency, divergence detection,
//! per-table scoping and persistence — through the public `Table` API.

use stratodb::{
    index::{IndexColumn, IndexDef},
    path::SPath,
    SdbError,
    StratoDb,
};

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
