//! Integration coverage for the paths driven only in specific regimes: an entity
//! packed into one blob (read/patch/unpack), an index reaching into a collection
//! (which keeps it shredded, so a query recomposes each entity from its own
//! subtree), and paths resolved relative to a rooted view.

use stratodb::{
    data::{Scalar, Seq},
    index::{IndexColumn, IndexDef},
    path::SPath,
    NodeKind,
    SdbError,
    StratoDb,
    Table,
    Value,
};

use std::collections::BTreeMap;

fn table() -> Table {
    StratoDb::create_in_memory()
        .expect("create db")
        .open_table("data")
        .expect("open table")
}

fn mixed_entity() -> Value {
    let mut entity = Value::new_empty_node();
    entity.set_value("a", Value::new_leaf(Scalar::I32(1)));
    entity.set_value("s", Value::new_leaf(Scalar::Str(String::from("hi"))));

    entity
}

#[test]
fn packed_entity_read_patch_and_reload() {
    let table = table();

    // Stored where no index reaches — so the whole subtree packs into one blob.
    let w = table.write().unwrap();
    w.store_value("doc/entity", &mixed_entity()).unwrap();
    w.commit().unwrap();

    // Zero-copy field reads out of the packed blob.
    let r = table.read().unwrap();
    assert_eq!(r.kind("doc/entity").unwrap(), Some(NodeKind::Object));
    assert_eq!(r.get::<i32>("doc/entity/a").unwrap(), Some(1));
    assert_eq!(
        r.get_scalar("doc/entity/s").unwrap(),
        Some(Scalar::Str(String::from("hi")))
    );
    assert!(r.get::<i32>("doc/entity/absent").unwrap().is_none());

    // Patch the packed entity three ways: a same-length scalar (spliced in place),
    // a different-length scalar (read-modify-write), and a brand-new field.
    let w = table.write().unwrap();
    w.put("doc/entity/a", &999i32).unwrap();
    w.put("doc/entity/s", &String::from("hello world")).unwrap();
    w.put("doc/entity/added", &7i32).unwrap();
    assert!(w.remove("doc/entity/a").unwrap());
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert!(r.get::<i32>("doc/entity/a").unwrap().is_none());
    assert_eq!(
        r.get::<String>("doc/entity/s").unwrap(),
        Some(String::from("hello world"))
    );
    assert_eq!(r.get::<i32>("doc/entity/added").unwrap(), Some(7));

    // Dynamic load of the packed entity, and of its (shredded) parent — the parent
    // walk meets the packed child and decodes it in place.
    let entity = r.load_value("doc/entity").unwrap().unwrap();
    assert_eq!(entity.get_value("added"), Some(Value::new_leaf(Scalar::I32(7))));

    let parent = r.load_value("doc").unwrap().unwrap();
    assert_eq!(parent.get_value("entity/added"), Some(Value::new_leaf(Scalar::I32(7))));

    // A path that descends into the packed entity but resolves to nothing.
    assert!(r.load_value("doc/entity/absent").unwrap().is_none());

    // A typed load of a field *inside* the packed entity (the walk stops at the
    // entity, then resolves the remainder within its blob).
    assert_eq!(r.load::<i32>("doc/entity/added").unwrap(), 7);
}

#[test]
fn reads_before_any_write_are_absent() {
    let table = table();

    // The engine table does not exist until the first write, so every read short-
    // circuits: scalars/kinds are `None`, whole-value loads are `PathNotFound`.
    let r = table.read().unwrap();
    assert!(r.get::<i32>("x").unwrap().is_none());
    assert!(r.get_scalar("x").unwrap().is_none());
    assert!(r.kind("x").unwrap().is_none());
    assert!(!r.exists("x").unwrap());
    assert!(matches!(r.load::<i32>("x"), Err(SdbError::PathNotFound(_))));
    assert!(matches!(r.fetch::<Seq<i32>>("x"), Err(SdbError::PathNotFound(_))));

    // An index query against a never-written table yields nothing.
    table
        .create_index(&IndexDef::new(
            String::from("i"),
            String::from("x/*"),
            vec![IndexColumn::asc(SPath::parse("a").unwrap())],
            false,
        ))
        .unwrap();

    let r = table.read().unwrap();
    assert!(
        r.find::<BTreeMap<String, i32>>("i", &[Scalar::I32(1)])
            .unwrap()
            .is_empty()
    );
}

#[test]
fn storing_at_the_table_root_replaces_the_whole_tree() {
    let table = table();

    let w = table.write().unwrap();
    w.put("keep/a", &1i32).unwrap();
    w.commit().unwrap();

    // Storing at the empty (root) path anchors nowhere — it replaces everything.
    let w = table.write().unwrap();
    w.store("", &vec![10i32, 20]).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.load::<Vec<i32>>("").unwrap(), vec![10, 20]);
    assert!(r.get::<i32>("keep/a").unwrap().is_none());
}

#[test]
fn overwriting_a_packed_entity_keeps_it_a_single_node() {
    let table = table();

    let w = table.write().unwrap();
    w.store_value("doc", &mixed_entity()).unwrap();
    w.commit().unwrap();

    // A replacing store rewrites the one blob in place.
    let mut replacement = Value::new_empty_node();
    replacement.set_value("a", Value::new_leaf(Scalar::I32(42)));

    let w = table.write().unwrap();
    w.store_value("doc", &replacement).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.get::<i32>("doc/a").unwrap(), Some(42));
    assert!(r.get::<String>("doc/s").unwrap().is_none());
}

#[test]
fn packed_list_reads_and_patches_one_element() {
    let table = table();

    let w = table.write().unwrap();
    w.store("nums", &vec![10i32, 20, 30, 40]).unwrap();
    w.commit().unwrap();

    // A packed list navigates zero-copy to one element.
    let r = table.read().unwrap();
    assert_eq!(r.kind("nums").unwrap(), Some(NodeKind::List));
    assert_eq!(r.get::<i32>("nums[2]").unwrap(), Some(30));
    assert_eq!(r.load::<Vec<i32>>("nums").unwrap(), vec![10, 20, 30, 40]);

    let w = table.write().unwrap();
    w.put("nums[1]", &999i32).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.load::<Vec<i32>>("nums").unwrap(), vec![10, 999, 30, 40]);
}

#[test]
fn an_index_reaching_into_a_collection_keeps_it_shredded() {
    let table = table();

    // `users/*` reaches strictly below `users`, so storing the whole collection
    // leaves every entity shredded (each child keeps its own key for the index).
    table
        .create_index(&IndexDef::new(
            String::from("by_age"),
            String::from("users/*"),
            vec![IndexColumn::asc(SPath::parse("age").unwrap())],
            false,
        ))
        .unwrap();

    let everyone: BTreeMap<String, BTreeMap<String, i32>> = BTreeMap::from([
        (String::from("alice"), BTreeMap::from([(String::from("age"), 30)])),
        (String::from("bob"), BTreeMap::from([(String::from("age"), 40)])),
        (String::from("carol"), BTreeMap::from([(String::from("age"), 30)])),
    ]);

    let w = table.write().unwrap();
    w.store("users", &everyone).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    // Shredded, not packed: the entity node is a plain object.
    assert_eq!(r.kind("users/alice").unwrap(), Some(NodeKind::Object));
    assert_eq!(r.get::<i32>("users/alice/age").unwrap(), Some(30));

    // A query recomposes each shredded entity by re-rooting a reader at its key.
    let mut found = r.find::<BTreeMap<String, i32>>("by_age", &[Scalar::I32(30)]).unwrap();
    found.sort_by_key(|m| m["age"]);
    assert_eq!(found.len(), 2);
    assert_eq!(found[0], BTreeMap::from([(String::from("age"), 30)]));

    // A reverse full scan and a whole-index scan both exercise the recompose path.
    let all = r.query("by_age").reversed().run::<BTreeMap<String, i32>>().unwrap();
    assert_eq!(all.len(), 3);

    // Updating one element's leaf rewrites just that node (the shredded regime).
    let w = table.write().unwrap();
    w.put("users/alice/age", &41i32).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(
        r.find::<BTreeMap<String, i32>>("by_age", &[Scalar::I32(30)])
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_deeper_index_forces_a_shredded_entity_recompose() {
    let table = table();

    // Index A points at each user (`users/*`); index B reaches strictly *below* a
    // user (`users/*/rank`), which keeps the user node shredded even though a whole
    // user is a natural pack unit. Recomposing an A-match then re-roots a plain
    // reader at the shredded entity rather than decoding a packed blob.
    table
        .create_index(&IndexDef::new(
            String::from("by_age"),
            String::from("users/*"),
            vec![IndexColumn::asc(SPath::parse("age").unwrap())],
            false,
        ))
        .unwrap();
    table
        .create_index(&IndexDef::new(
            String::from("by_rank"),
            String::from("users/*/rank"),
            vec![IndexColumn::asc(SPath::root())],
            false,
        ))
        .unwrap();

    let everyone: BTreeMap<String, BTreeMap<String, i32>> = BTreeMap::from([
        (
            String::from("alice"),
            BTreeMap::from([(String::from("age"), 30), (String::from("rank"), 1)]),
        ),
        (
            String::from("bob"),
            BTreeMap::from([(String::from("age"), 40), (String::from("rank"), 2)]),
        ),
    ]);

    let w = table.write().unwrap();
    w.store("users", &everyone).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    let found = r.find::<BTreeMap<String, i32>>("by_age", &[Scalar::I32(30)]).unwrap();
    assert_eq!(
        found,
        vec![BTreeMap::from([(String::from("age"), 30), (String::from("rank"), 1)])]
    );
}

#[test]
fn rooted_views_resolve_relative_paths() {
    let table = table();

    // RootedWrite: absolute writes anchored at a root, including sub-views.
    let w = table.write().unwrap();
    {
        let alice = w.rooted("users/alice").unwrap();
        assert_eq!(alice.root().to_string(), "users/alice");

        alice.put("age", &30i32).unwrap();
        alice.store("profile", &vec![1i32, 2, 3]).unwrap();

        let profile = alice.rooted("profile").unwrap();
        assert_eq!(profile.kind("").unwrap(), Some(NodeKind::List));
        assert_eq!(profile.load::<Vec<i32>>("").unwrap(), vec![1, 2, 3]);

        assert!(!alice.remove("gone").unwrap());
    }
    w.commit().unwrap();

    // RootedRead: relative reads and a scoped index find.
    let r = table.read().unwrap();
    let alice = r.rooted("users/alice").unwrap();
    assert_eq!(alice.root().to_string(), "users/alice");
    assert_eq!(alice.get::<i32>("age").unwrap(), Some(30));
    assert_eq!(alice.kind("profile").unwrap(), Some(NodeKind::List));
    assert!(alice.exists("age").unwrap());
    assert_eq!(alice.load::<Vec<i32>>("profile").unwrap(), vec![1, 2, 3]);

    let sub = alice.rooted("profile").unwrap();
    assert_eq!(sub.get::<i32>("[0]").unwrap(), Some(1));
}

#[test]
fn a_scoped_query_prunes_to_the_rooted_subtree() {
    let table = table();

    table
        .create_index(&IndexDef::new(
            String::from("by_age"),
            String::from("users/*"),
            vec![IndexColumn::asc(SPath::parse("age").unwrap())],
            false,
        ))
        .unwrap();

    let w = table.write().unwrap();
    w.put("users/alice/age", &30i32).unwrap();
    w.put("users/bob/age", &30i32).unwrap();
    w.commit().unwrap();

    // The view restricts the table-global index to entities under its root.
    let r = table.read().unwrap();
    let scoped = r.rooted("users/alice").unwrap();
    let found = scoped
        .find::<BTreeMap<String, i32>>("by_age", &[Scalar::I32(30)])
        .unwrap();
    assert_eq!(found, vec![BTreeMap::from([(String::from("age"), 30)])]);
}
