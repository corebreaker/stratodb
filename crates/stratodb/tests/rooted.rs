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
    let db = StratoDb::create_in_memory().unwrap();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.put("users/alice/age", &30i32).unwrap();
    w.put("users/alice/name", &String::from("Alice")).unwrap();
    w.put("users/alice/scores/math", &90i32).unwrap();
    w.put("users/alice/scores/art", &75i32).unwrap();
    w.put("users/bob/age", &40i32).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    let alice = r.rooted("users/alice").unwrap();

    assert_eq!(alice.get::<i32>("age").unwrap(), Some(30));
    assert_eq!(alice.get::<String>("name").unwrap(), Some(String::from("Alice")));
    assert_eq!(alice.get_scalar("age").unwrap(), Some(Scalar::I32(30)));
    assert!(alice.exists("age").unwrap());
    assert!(!alice.exists("missing").unwrap());

    // `rooted` also accepts an already-built `SPath`, by value or by reference.
    let by_path = root("users/alice");
    assert_eq!(r.rooted(&by_path).unwrap().get::<i32>("age").unwrap(), Some(30));
    assert_eq!(r.rooted(by_path).unwrap().get::<i32>("age").unwrap(), Some(30));

    // A leaf accessor, fetched relative to the root.
    assert_eq!(alice.fetch::<Leaf<i32>>("age").unwrap().get().unwrap(), 30);

    // Nesting descends further; `load("")` recomposes the node at the root itself.
    let scores = alice.rooted("scores").unwrap();
    assert_eq!(
        scores.load::<BTreeMap<String, i32>>("").unwrap(),
        BTreeMap::from([(String::from("art"), 75), (String::from("math"), 90)])
    );
}

#[test]
fn rooted_write_is_relative_and_persists() {
    let db = StratoDb::create_in_memory().unwrap();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    {
        let alice = w.rooted("users/alice").unwrap();
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
    assert_eq!(r.rooted("users/alice").unwrap().get::<i32>("age").unwrap(), Some(31));
}

#[test]
fn rooted_writes_maintain_indexes() {
    let db = StratoDb::create_in_memory().unwrap();
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
        let view = w.rooted("users").unwrap();
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

#[test]
fn rooted_find_scopes_to_the_subtree() {
    let db = StratoDb::create_in_memory().unwrap();
    let org = db.open_table("org").unwrap();

    org.create_index(&IndexDef::new(
        String::from("by_age"),
        String::from("org/*/members/*"),
        vec![IndexColumn::asc(root("age"))],
        false,
    ))
    .unwrap();

    let w = org.write().unwrap();
    w.put("org/eng/members/alice/age", &30i32).unwrap();
    w.put("org/eng/members/bob/age", &40i32).unwrap();
    w.put("org/sales/members/carol/age", &30i32).unwrap();
    w.commit().unwrap();

    let r = org.read().unwrap();
    let count = |v: stratodb::txn::RootedRead<'_>| {
        v.find::<BTreeMap<String, i32>>("by_age", &[Scalar::I32(30)])
            .unwrap()
            .len()
    };

    // Unrooted: both 30-year-olds across the whole table.
    assert_eq!(
        r.find::<BTreeMap<String, i32>>("by_age", &[Scalar::I32(30)])
            .unwrap()
            .len(),
        2,
    );

    // Rooted at a team: only that team's matching members.
    assert_eq!(count(r.rooted("org/eng").unwrap()), 1);
    assert_eq!(count(r.rooted("org/sales").unwrap()), 1);

    // Rooted above the teams: all of them; nesting composes the same way.
    assert_eq!(count(r.rooted("org").unwrap()), 2);
    assert_eq!(count(r.rooted("org").unwrap().rooted("eng").unwrap()), 1);

    // Rooted at a single entity: that entity when it matches, otherwise none.
    let alice = r.rooted("org/eng/members/alice").unwrap();
    assert_eq!(
        alice
            .find::<BTreeMap<String, i32>>("by_age", &[Scalar::I32(30)])
            .unwrap()
            .len(),
        1,
    );
    assert_eq!(
        alice
            .find::<BTreeMap<String, i32>>("by_age", &[Scalar::I32(40)])
            .unwrap()
            .len(),
        0,
    );

    // Rooted below the entity depth, or on a non-matching entity: nothing.
    assert_eq!(count(r.rooted("org/eng/members/alice/age").unwrap()), 0);
    assert_eq!(count(r.rooted("org/eng/members/bob").unwrap()), 0);
}

#[test]
fn rooted_query_builder_scopes_and_reverses() {
    let db = StratoDb::create_in_memory().unwrap();
    let org = db.open_table("org").unwrap();

    org.create_index(&IndexDef::new(
        String::from("by_age"),
        String::from("org/*/members/*"),
        vec![IndexColumn::asc(root("age"))],
        false,
    ))
    .unwrap();

    let w = org.write().unwrap();
    w.put("org/eng/members/alice/age", &30i32).unwrap();
    w.put("org/eng/members/bob/age", &40i32).unwrap();
    w.put("org/sales/members/carol/age", &30i32).unwrap();
    w.commit().unwrap();

    let r = org.read().unwrap();
    let ages = |hits: Vec<BTreeMap<String, i32>>| hits.into_iter().map(|m| m["age"]).collect::<Vec<_>>();
    let eng = r.rooted("org/eng").unwrap();

    // The view's query builder is scoped to the root: only eng members, and
    // `reverse` flips the index order (descending by age).
    assert_eq!(ages(eng.query("by_age").reversed().run().unwrap()), vec![40, 30]);

    // A scoped prefix keeps only eng's age-30 member (sales' carol is excluded).
    assert_eq!(
        ages(eng.query("by_age").prefixed(&[Scalar::I32(30)]).run().unwrap()),
        vec![30]
    );
}
