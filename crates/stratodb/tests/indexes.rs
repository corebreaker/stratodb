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
    let db = StratoDb::create_in_memory().unwrap();
    let users = db.open_table("users").unwrap();

    users.create_index(&def("by_age_name", false)).unwrap();
    users.create_index(&def("by_age_name", false)).unwrap(); // identical -> idempotent

    assert_eq!(users.index_def("by_age_name").unwrap(), Some(def("by_age_name", false)));
    assert!(users.index_def("missing").unwrap().is_none());
}

#[test]
fn create_index_rejects_a_divergent_redefinition() {
    let db = StratoDb::create_in_memory().unwrap();
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
    let db = StratoDb::create_in_memory().unwrap();
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
    let db = StratoDb::create_in_memory().unwrap();
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
    let db = StratoDb::create_in_memory().unwrap();
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
    let db = StratoDb::create_in_memory().unwrap();
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
fn composite_index_supports_exact_and_prefix_match() {
    let db = StratoDb::create_in_memory().unwrap();
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
        w.put(format!("users/{name}/a"), &a).unwrap();
        w.put(format!("users/{name}/b"), &b).unwrap();
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

    // A leading-column prefix matches every entity with that `a` (any `b`).
    assert_eq!(
        r.find::<BTreeMap<String, i32>>("by_a_b", &[Scalar::I32(1)])
            .unwrap()
            .len(),
        2
    );

    // More values than the index has columns is still an error.
    let arity = r
        .find::<BTreeMap<String, i32>>("by_a_b", &[Scalar::I32(1), Scalar::I32(5), Scalar::I32(9)])
        .unwrap_err();

    assert!(matches!(arity, SdbError::IndexArity { .. }), "got {arity:?}");
}

#[test]
fn unique_index_keeps_the_entity_in_the_value() {
    let db = StratoDb::create_in_memory().unwrap();
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

#[test]
fn query_builder_does_prefix_reverse_and_full_scans() {
    let db = StratoDb::create_in_memory().unwrap();
    let users = db.open_table("users").unwrap();
    users.create_index(&single("by_age", "age", false)).unwrap();

    let w = users.write().unwrap();
    for (name, age) in [("alice", 30), ("bob", 20), ("carol", 30), ("dave", 40)] {
        w.put(format!("users/{name}/age"), &age).unwrap();
    }
    w.commit().unwrap();

    let r = users.read().unwrap();
    let ages = |hits: Vec<BTreeMap<String, i32>>| hits.into_iter().map(|m| m["age"]).collect::<Vec<_>>();

    // Empty prefix = every indexed entity, in ascending index (age) order.
    assert_eq!(ages(r.query("by_age").run().unwrap()), vec![20, 30, 30, 40]);

    // Reverse = descending index order.
    assert_eq!(ages(r.query("by_age").reversed().run().unwrap()), vec![40, 30, 30, 20]);

    // A prefix narrows to one value (single column); two entities are 30.
    assert_eq!(
        ages(r.query("by_age").prefixed(&[Scalar::I32(30)]).run().unwrap()),
        vec![30, 30]
    );

    // `find` is the ascending exact/prefix shortcut.
    assert_eq!(ages(r.find("by_age", &[Scalar::I32(40)]).unwrap()), vec![40]);
}

#[test]
fn unique_index_rejects_duplicates() {
    let db = StratoDb::create_in_memory().unwrap();
    let users = db.open_table("users").unwrap();
    users.create_index(&single("uby_age", "age", true)).unwrap();

    // Seed alice = 30.
    let w = users.write().unwrap();
    w.put("users/alice/age", &30i32).unwrap();
    w.commit().unwrap();

    // A second entity with the same value is rejected, and its write rolls back.
    let w = users.write().unwrap();
    let err = w.put("users/bob/age", &30i32).unwrap_err();
    assert!(matches!(err, SdbError::UniqueViolation { .. }), "got {err:?}");
    drop(w);
    assert!(!users.read().unwrap().exists("users/bob").unwrap());

    // A distinct value is fine; later moving it onto a taken value is rejected.
    let w = users.write().unwrap();
    w.put("users/bob/age", &40i32).unwrap();
    w.commit().unwrap();

    let w = users.write().unwrap();
    assert!(matches!(
        w.put("users/bob/age", &30i32).unwrap_err(),
        SdbError::UniqueViolation { .. }
    ));
    drop(w);

    // Re-storing an entity's own value is not a self-violation.
    let w = users.write().unwrap();
    w.put("users/alice/age", &30i32).unwrap();
    w.commit().unwrap();

    // A bulk store carrying an in-batch duplicate is rejected too.
    let dups: BTreeMap<String, BTreeMap<String, i32>> = BTreeMap::from([
        (String::from("x"), BTreeMap::from([(String::from("age"), 7)])),
        (String::from("y"), BTreeMap::from([(String::from("age"), 7)])),
    ]);

    let w = users.write().unwrap();
    assert!(matches!(
        w.store("users", &dups).unwrap_err(),
        SdbError::UniqueViolation { .. }
    ));
    drop(w);

    // Final committed state is exactly alice = 30 and bob = 40.
    let r = users.read().unwrap();
    assert_eq!(
        r.find::<BTreeMap<String, i32>>("uby_age", &[Scalar::I32(30)])
            .unwrap()
            .len(),
        1
    );

    assert_eq!(
        r.find::<BTreeMap<String, i32>>("uby_age", &[Scalar::I32(40)])
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn create_index_backfills_existing_data() {
    let db = StratoDb::create_in_memory().unwrap();
    let users = db.open_table("users").unwrap();

    // Populate the table *before* the index exists.
    let w = users.write().unwrap();
    w.put("users/alice/age", &30i32).unwrap();
    w.put("users/bob/age", &30i32).unwrap();
    w.put("users/carol/age", &40i32).unwrap();
    w.commit().unwrap();

    // Creating the index now back-fills those pre-existing rows.
    users.create_index(&single("by_age", "age", false)).unwrap();
    assert_eq!(count_at(&db, "users", "by_age", 30), 2);
    assert_eq!(count_at(&db, "users", "by_age", 40), 1);

    // And later writes keep maintaining it.
    let w = users.write().unwrap();
    w.put("users/dave/age", &30i32).unwrap();
    w.commit().unwrap();
    assert_eq!(count_at(&db, "users", "by_age", 30), 3);
}

#[test]
fn has_index_reports_presence_scoped_per_table() {
    let db = StratoDb::create_in_memory().unwrap();
    let users = db.open_table("users").unwrap();

    assert!(!users.has_index("by_age").unwrap());

    users.create_index(&single("by_age", "age", false)).unwrap();
    assert!(users.has_index("by_age").unwrap());
    assert!(!users.has_index("missing").unwrap());

    // Like `index_def`, presence is per table.
    assert!(!db.open_table("posts").unwrap().has_index("by_age").unwrap());
}

#[test]
fn delete_index_removes_registration_and_clears_queries() {
    let db = StratoDb::create_in_memory().unwrap();
    let users = db.open_table("users").unwrap();
    users.create_index(&single("by_age", "age", false)).unwrap();

    let w = users.write().unwrap();
    w.put("users/alice/age", &30i32).unwrap();
    w.put("users/bob/age", &30i32).unwrap();
    w.commit().unwrap();
    assert_eq!(count_at(&db, "users", "by_age", 30), 2);

    // Dropping it reports success and clears every trace from the registry.
    assert!(users.delete_index("by_age").unwrap());
    assert!(!users.has_index("by_age").unwrap());
    assert!(users.index_def("by_age").unwrap().is_none());

    // The index no longer resolves: a query against it errors.
    let err = users
        .read()
        .unwrap()
        .find::<BTreeMap<String, i32>>("by_age", &[Scalar::I32(30)])
        .unwrap_err();
    assert!(matches!(err, SdbError::IndexNotFound { .. }), "got {err:?}");

    // The indexed data itself is untouched.
    assert_eq!(users.read().unwrap().get::<i32>("users/alice/age").unwrap(), Some(30));

    // Dropping a missing index is a no-op (idempotent).
    assert!(!users.delete_index("by_age").unwrap());
}

#[test]
fn delete_index_leaves_other_indexes_and_data_intact() {
    let db = StratoDb::create_in_memory().unwrap();
    let users = db.open_table("users").unwrap();

    // Two indexes over a uniform (all-i32) entity, so `find` recomposes cleanly.
    users.create_index(&single("by_age", "age", false)).unwrap();
    users.create_index(&single("by_rank", "rank", false)).unwrap();

    let w = users.write().unwrap();
    w.put("users/alice/age", &30i32).unwrap();
    w.put("users/alice/rank", &1i32).unwrap();
    w.put("users/bob/age", &30i32).unwrap();
    w.put("users/bob/rank", &2i32).unwrap();
    w.commit().unwrap();

    // Drop only `by_age`; its purge must not touch `by_rank`'s entries or any node.
    assert!(users.delete_index("by_age").unwrap());

    let r = users.read().unwrap();
    assert!(matches!(
        r.find::<BTreeMap<String, i32>>("by_age", &[Scalar::I32(30)])
            .unwrap_err(),
        SdbError::IndexNotFound { .. }
    ));

    // The surviving index still resolves each entity by its distinct rank.
    assert_eq!(
        r.find::<BTreeMap<String, i32>>("by_rank", &[Scalar::I32(1)])
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        r.find::<BTreeMap<String, i32>>("by_rank", &[Scalar::I32(2)])
            .unwrap()
            .len(),
        1
    );

    // Nodes are intact.
    assert_eq!(r.get::<i32>("users/bob/age").unwrap(), Some(30));
}

#[test]
fn delete_index_then_recreate_rebuilds_without_stale_entries() {
    let db = StratoDb::create_in_memory().unwrap();
    let users = db.open_table("users").unwrap();
    users.create_index(&single("by_age", "age", false)).unwrap();

    let w = users.write().unwrap();
    w.put("users/alice/age", &30i32).unwrap();
    w.put("users/bob/age", &30i32).unwrap();
    w.commit().unwrap();

    users.delete_index("by_age").unwrap();

    // A fresh index under the same name back-fills cleanly: exactly two hits, not
    // doubled by entries the drop should have purged.
    users.create_index(&single("by_age", "age", false)).unwrap();
    assert_eq!(count_at(&db, "users", "by_age", 30), 2);
}

#[test]
fn dropping_a_unique_index_frees_its_collisions() {
    let db = StratoDb::create_in_memory().unwrap();
    let users = db.open_table("users").unwrap();
    users.create_index(&single("uby_age", "age", true)).unwrap();

    let w = users.write().unwrap();
    w.put("users/alice/age", &30i32).unwrap();
    w.commit().unwrap();

    // While the unique index stands, a colliding value is rejected.
    let w = users.write().unwrap();
    assert!(matches!(
        w.put("users/bob/age", &30i32).unwrap_err(),
        SdbError::UniqueViolation { .. }
    ));
    drop(w);

    // Dropping the index lifts the constraint: the duplicate now stores fine.
    assert!(users.delete_index("uby_age").unwrap());

    let w = users.write().unwrap();
    w.put("users/bob/age", &30i32).unwrap();
    w.commit().unwrap();
    assert_eq!(users.read().unwrap().get::<i32>("users/bob/age").unwrap(), Some(30));
}

#[test]
fn deleted_index_stays_gone_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("idx_delete_reopen.stratodb");

    {
        let db = StratoDb::create(&path).unwrap();
        let users = db.open_table("users").unwrap();
        users.create_index(&def("by_age_name", false)).unwrap();
        assert!(users.delete_index("by_age_name").unwrap());
    }

    let db = StratoDb::open(&path).unwrap();
    assert!(!db.open_table("users").unwrap().has_index("by_age_name").unwrap());
}

#[test]
fn ensure_index_creates_when_absent_and_is_a_noop_when_present() {
    let db = StratoDb::create_in_memory().unwrap();
    let users = db.open_table("users").unwrap();

    // Data exists before the index.
    let w = users.write().unwrap();
    w.put("users/alice/age", &30i32).unwrap();
    w.put("users/bob/age", &30i32).unwrap();
    w.commit().unwrap();

    // Absent -> created and back-filled, exactly like `create_index`.
    users.ensure_index(&single("by_age", "age", false)).unwrap();
    assert!(users.has_index("by_age").unwrap());
    assert_eq!(count_at(&db, "users", "by_age", 30), 2);

    // Present (identical) -> no error, no second back-fill.
    users.ensure_index(&single("by_age", "age", false)).unwrap();
    assert_eq!(count_at(&db, "users", "by_age", 30), 2);
}

#[test]
fn ensure_index_leaves_a_divergent_existing_index_untouched() {
    let db = StratoDb::create_in_memory().unwrap();
    let users = db.open_table("users").unwrap();

    users.create_index(&def("by_age_name", false)).unwrap();

    // A divergent definition under the same name: `create_index` would error,
    // but `ensure_index` quietly leaves the original in place (no reconciliation).
    users.ensure_index(&def("by_age_name", true)).unwrap(); // unique flag flipped
    users.ensure_index(&single("by_age_name", "age", false)).unwrap(); // different columns

    assert_eq!(users.index_def("by_age_name").unwrap(), Some(def("by_age_name", false)));
}

#[test]
fn creating_a_unique_index_over_duplicates_is_rejected() {
    let db = StratoDb::create_in_memory().unwrap();
    let users = db.open_table("users").unwrap();

    // Two pre-existing rows share a value.
    let w = users.write().unwrap();
    w.put("users/alice/age", &30i32).unwrap();
    w.put("users/bob/age", &30i32).unwrap();
    w.commit().unwrap();

    // Back-filling a unique index over them fails — and the whole creation rolls
    // back, leaving no index registered.
    let err = users.create_index(&single("uby_age", "age", true)).unwrap_err();
    assert!(matches!(err, SdbError::UniqueViolation { .. }), "got {err:?}");
    assert!(users.index_def("uby_age").unwrap().is_none());
}
