//! Integration tests for the storage foundation: the untyped node tree, paths,
//! transactions, cascade deletes and persistence.

use stratodb::{error::SdbError, data::Scalar, path::SPath, NodeKind, StratoDb};

fn mem_db() -> StratoDb {
    StratoDb::create_in_memory().expect("create db")
}

#[test]
fn store_many_writes_every_pair() {
    let db = mem_db();
    let table = db.open_table("data").unwrap();

    // Owned values kept in scope so the pairs can borrow them.
    let (x, y, z) = (1i32, 2i32, 3i32);
    let pairs = vec![
        (SPath::parse("users/a/score").unwrap(), &x),
        (SPath::parse("users/b/score").unwrap(), &y),
        (SPath::parse("users/c/score").unwrap(), &z),
    ];

    let w = table.write().unwrap();
    w.store_many(&pairs).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.get::<i32>("users/a/score").unwrap(), Some(1));
    assert_eq!(r.get::<i32>("users/b/score").unwrap(), Some(2));
    assert_eq!(r.get::<i32>("users/c/score").unwrap(), Some(3));
    assert_eq!(r.load::<i32>("users/c/score").unwrap(), 3);
}

#[test]
fn put_and_get_across_objects_and_lists() {
    let db = mem_db();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.put("a/b/c", &42u32).unwrap();
    w.put("a/t[0]", &String::from("zero")).unwrap();
    w.put("a/t[1]", &String::from("one")).unwrap();
    // value visible within the same write transaction
    assert_eq!(w.get::<u32>("a/b/c").unwrap(), Some(42));
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.get::<u32>("a/b/c").unwrap(), Some(42));
    assert_eq!(r.get::<String>("a/t[0]").unwrap(), Some("zero".into()));
    assert_eq!(r.get::<String>("a/t[1]").unwrap(), Some("one".into()));

    assert_eq!(r.kind("a").unwrap(), Some(NodeKind::Object));
    assert_eq!(r.kind("a/b").unwrap(), Some(NodeKind::Object));
    assert_eq!(r.kind("a/t").unwrap(), Some(NodeKind::List));
    assert_eq!(r.kind("a/b/c").unwrap(), Some(NodeKind::Leaf));
    assert_eq!(r.kind("missing").unwrap(), None);
    assert!(r.exists("a/t[1]").unwrap());
    assert!(!r.exists("a/t[5]").unwrap());
}

#[test]
fn replace_semantics_overwrite_subtree() {
    let db = mem_db();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.put("a/b/c", &1u32).unwrap();
    w.put("a/b/d", &2u32).unwrap();
    // overwrite scalar in place
    w.put("a/b/c", &100u32).unwrap();
    // replace the whole object at a/b with a scalar
    w.put("a/b", &7u32).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.get::<u32>("a/b").unwrap(), Some(7));
    assert_eq!(r.kind("a/b").unwrap(), Some(NodeKind::Leaf));
    // the old children are gone
    assert_eq!(r.kind("a/b/c").unwrap(), None);
    assert_eq!(r.kind("a/b/d").unwrap(), None);
}

#[test]
fn remove_cascades() {
    let db = mem_db();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.put("a/b/c", &1u32).unwrap();
    w.put("a/t[0]", &10u32).unwrap();
    assert!(w.remove("a").unwrap());
    assert!(!w.remove("a").unwrap()); // already gone
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.kind("a").unwrap(), None);
    assert_eq!(r.kind("a/b/c").unwrap(), None);
    assert!(!r.exists("a/t[0]").unwrap());
}

#[test]
fn list_element_removal_reindexes_following_elements() {
    let db = mem_db();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.put("L[0]/name", &String::from("a")).unwrap();
    w.put("L[1]/name", &String::from("b")).unwrap();
    w.put("L[2]/name", &String::from("c")).unwrap();
    w.commit().unwrap();

    let w = table.write().unwrap();
    assert!(w.remove("L[1]").unwrap());
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.get::<String>("L[0]/name").unwrap(), Some("a".into()));
    // the former L[2] subtree shifted down to L[1]
    assert_eq!(r.get::<String>("L[1]/name").unwrap(), Some("c".into()));
    assert_eq!(r.kind("L[2]").unwrap(), None);
}

#[test]
fn type_mismatch_is_reported() {
    let db = mem_db();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.put("n", &42u32).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    let err = r.get::<String>("n").unwrap_err();
    assert!(matches!(err, SdbError::TypeMismatch { .. }), "got {err:?}");
}

#[test]
fn reading_a_container_as_a_scalar_errors() {
    let db = mem_db();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.put("a/b", &1u32).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    let err = r.get_scalar("a").unwrap_err();
    assert!(matches!(err, SdbError::UnexpectedNode { .. }), "got {err:?}");
}

#[test]
fn scalar_variety_roundtrips_through_storage() {
    let db = mem_db();
    let table = db.open_table("data").unwrap();

    let values = [
        ("b", Scalar::Bool(true)),
        ("i", Scalar::I64(-7)),
        ("f", Scalar::F64(3.5)),
        ("s", Scalar::Str("hi".into())),
        ("z", Scalar::Null),
    ];

    let w = table.write().unwrap();
    for (path, scalar) in &values {
        w.put_scalar(*path, scalar.clone()).unwrap();
    }
    w.commit().unwrap();

    let r = table.read().unwrap();
    for (path, scalar) in &values {
        assert_eq!(r.get_scalar(*path).unwrap().as_ref(), Some(scalar));
    }
}

#[test]
fn data_persists_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("persist.stratodb");

    {
        let db = StratoDb::create(&path).unwrap();
        let table = db.open_table("data").unwrap();
        let w = table.write().unwrap();
        w.put("greeting", &String::from("hello")).unwrap();
        w.commit().unwrap();
    }

    let db = StratoDb::open(&path).unwrap();
    let table = db.open_table("data").unwrap();
    let r = table.read().unwrap();
    assert_eq!(r.get::<String>("greeting").unwrap(), Some("hello".into()));
}

#[test]
fn tables_are_isolated() {
    let db = mem_db();
    let users = db.open_table("users").unwrap();
    let orders = db.open_table("orders").unwrap();

    let w = users.write().unwrap();
    w.put("alice/age", &30u32).unwrap();
    w.commit().unwrap();

    // the other table does not see the first table's data
    let r = orders.read().unwrap();
    assert_eq!(r.kind("alice").unwrap(), None);
}

#[test]
fn reserved_and_empty_table_names_are_rejected() {
    let db = mem_db();
    assert!(db.open_table("$metadata").is_err());
    assert!(db.open_table("").is_err());
}

#[test]
fn in_memory_db_roundtrips_committed_data() {
    let db = StratoDb::create_in_memory().unwrap();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.put("a/b/c", &42u32).unwrap();
    w.commit().unwrap();

    // survives the commit and is readable from a fresh read transaction
    let r = table.read().unwrap();
    assert_eq!(r.get::<u32>("a/b/c").unwrap(), Some(42));
}

#[test]
fn in_memory_dbs_are_independent() {
    let one = StratoDb::create_in_memory().unwrap();
    let two = StratoDb::create_in_memory().unwrap();

    let w = one.open_table("data").unwrap().write().unwrap();
    w.put("k", &1u32).unwrap();
    w.commit().unwrap();

    // a second in-memory database has its own storage and sees nothing
    let r = two.open_table("data").unwrap().read().unwrap();
    assert_eq!(r.kind("k").unwrap(), None);
}
