//! Rooted transaction views: `ReadTxn::rooted` / `WriteTxn::rooted` interpret
//! every path relative to a fixed root, and rooted writes still drive indexes.

use stratodb::{
    data::{
        leaf::{Leaf, LeafMut},
        Scalar,
    },
    index::{IndexColumn, IndexDef},
    path::SPath,
    StratoDb,
};

use std::collections::BTreeMap;

fn root(path: &str) -> SPath {
    SPath::parse(path).unwrap()
}

#[test]
fn rooted_read_resolves_relative_to_root() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("rr.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.put("users/alice/age", &30i32).unwrap();
    w.put("users/alice/name", &String::from("Alice")).unwrap();
    w.put("users/alice/scores/math", &90i32).unwrap();
    w.put("users/alice/scores/art", &75i32).unwrap();
    w.put("users/bob/age", &40i32).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    let alice = r.rooted(root("users/alice"));

    assert_eq!(alice.get::<i32>("age").unwrap(), Some(30));
    assert_eq!(alice.get::<String>("name").unwrap(), Some(String::from("Alice")));
    assert_eq!(alice.get_scalar("age").unwrap(), Some(Scalar::I32(30)));
    assert!(alice.exists("age").unwrap());
    assert!(!alice.exists("missing").unwrap());

    // A leaf accessor, fetched relative to the root.
    assert_eq!(alice.fetch::<Leaf<i32>>("age").unwrap().get().unwrap(), 30);

    // Nesting descends further; `load("")` recomposes the node at the root itself.
    let scores = alice.rooted(root("scores"));
    assert_eq!(
        scores.load::<BTreeMap<String, i32>>("").unwrap(),
        BTreeMap::from([(String::from("art"), 75), (String::from("math"), 90)])
    );
}

#[test]
fn rooted_write_is_relative_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("rw.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    {
        let alice = w.rooted(root("users/alice"));
        alice.put("age", &30i32).unwrap();
        alice
            .store("scores", &BTreeMap::from([(String::from("math"), 90i32)]))
            .unwrap();

        // Within the same transaction, the rooted view sees its own writes,
        let value: LeafMut<i32> = alice.fetch_mut("age").unwrap();
        value.set(&31i32).unwrap();

        assert_eq!(alice.get::<i32>("age").unwrap(), Some(31));
        assert_eq!(alice.get::<i32>("scores/math").unwrap(), Some(90));
        // and the same nodes are reachable by absolute path on the transaction.
        assert_eq!(w.get::<i32>("users/alice/age").unwrap(), Some(31));

        assert!(alice.remove("scores").unwrap());
        assert_eq!(alice.get::<i32>("scores/math").unwrap(), None);
    }
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.get::<i32>("users/alice/age").unwrap(), Some(31));
    assert_eq!(r.rooted(root("users/alice")).get::<i32>("age").unwrap(), Some(31));
}

#[test]
fn rooted_writes_maintain_indexes() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("rw_idx.stratodb")).unwrap();
    let users = db.open_table("users").unwrap();

    users
        .create_index(&IndexDef::new(
            String::from("by_age"),
            String::from("users/*"),
            vec![IndexColumn::asc(root("age"))],
            false,
        ))
        .unwrap();

    // Writing through a rooted view goes through the same maintenance path.
    let w = users.write().unwrap();
    {
        let view = w.rooted(root("users"));
        view.put("alice/age", &30i32).unwrap();
        view.put("bob/age", &30i32).unwrap();
    }
    w.commit().unwrap();

    let found = users
        .read()
        .unwrap()
        .find::<BTreeMap<String, i32>>("by_age", &[Scalar::I32(30)])
        .unwrap();
    assert_eq!(found.len(), 2);
}
