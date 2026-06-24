//! Container `SData` impls: `Vec`, `Option`, and the packed `Bytes` newtype,
//! exercised through both the value API (`store`/`load`) and the accessors.

use stratodb::{
    data::{Bytes, Map, MapMut, OptRef, Seq, SeqMut},
    NodeKind,
    StratoDb,
    Table,
};

use std::collections::BTreeMap;

fn table() -> (tempfile::TempDir, Table) {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = StratoDb::create(dir.path().join("containers.stratodb")).expect("create db");
    let table = db.open_table("data").expect("open table");
    (dir, table)
}

#[test]
fn vec_roundtrips_and_is_accessible() {
    let (_dir, table) = table();

    let w = table.write().unwrap();
    w.store("nums", &vec![10i32, 20, 30]).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.load::<Vec<i32>>("nums").unwrap(), vec![10, 20, 30]);

    let seq = r.fetch::<Seq<i32>>("nums").unwrap();
    assert_eq!(seq.len().unwrap(), 3);
    assert_eq!(seq.get(1).unwrap().get().unwrap(), 20);

    // Homogeneity: each element is a node reachable by raw path.
    assert_eq!(r.get::<i32>("nums[2]").unwrap(), Some(30));
}

#[test]
fn empty_vec_still_materializes_a_list_node() {
    let (_dir, table) = table();

    let w = table.write().unwrap();
    w.store("empty", &Vec::<i32>::new()).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.load::<Vec<i32>>("empty").unwrap(), Vec::<i32>::new());
    assert_eq!(r.kind("empty").unwrap(), Some(NodeKind::List));

    // The accessor works even though the list is empty.
    let seq = r.fetch::<Seq<i32>>("empty").unwrap();
    assert!(seq.is_empty().unwrap());
}

#[test]
fn option_some_and_none() {
    let (_dir, table) = table();

    let w = table.write().unwrap();
    w.store("some", &Some(7i32)).unwrap();
    w.store("none", &Option::<i32>::None).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.load::<Option<i32>>("some").unwrap(), Some(7));
    assert_eq!(r.load::<Option<i32>>("none").unwrap(), None);

    let some = r.fetch::<OptRef<i32>>("some").unwrap();
    assert!(!some.is_none().unwrap());
    assert_eq!(some.get().unwrap().unwrap().get().unwrap(), 7);

    let none = r.fetch::<OptRef<i32>>("none").unwrap();
    assert!(none.is_none().unwrap());
    assert!(none.get().unwrap().is_none());
}

#[test]
fn bytes_is_a_single_packed_leaf() {
    let (_dir, table) = table();

    let w = table.write().unwrap();
    w.store("blob", &Bytes(vec![1, 2, 3, 255])).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.load::<Bytes>("blob").unwrap(), Bytes(vec![1, 2, 3, 255]));
    // Packed: a single leaf, not a list of per-byte nodes.
    assert_eq!(r.kind("blob").unwrap(), Some(NodeKind::Leaf));
}

#[test]
fn seq_mut_push_appends() {
    let (_dir, table) = table();

    let w = table.write().unwrap();
    w.store("xs", &vec![1i32]).unwrap();

    let xs = w.fetch_mut::<SeqMut<i32>>("xs").unwrap();
    xs.push(&2).unwrap();
    xs.push(&3).unwrap();
    drop(xs); // the accessor borrows the transaction; release it before committing
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.load::<Vec<i32>>("xs").unwrap(), vec![1, 2, 3]);
}

#[test]
fn seq_mut_insert_and_remove() {
    let (_dir, table) = table();

    let w = table.write().unwrap();
    w.store("xs", &vec![1i32, 2, 3, 4, 5]).unwrap();
    {
        let xs = w.fetch_mut::<SeqMut<i32>>("xs").unwrap();
        xs.insert_at(0, &0).unwrap(); // [0, 1, 2, 3, 4, 5]
        xs.insert_at(3, &99).unwrap(); // [0, 1, 2, 99, 3, 4, 5]
        assert!(xs.remove_at(1).unwrap()); // [0, 2, 99, 3, 4, 5]
        xs.remove_range(2..4).unwrap(); // [0, 2, 4, 5]
    }
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.load::<Vec<i32>>("xs").unwrap(), vec![0, 2, 4, 5]);
}

#[test]
fn path_cache_stays_coherent_across_writes() {
    let (_dir, table) = table();

    let w = table.write().unwrap();
    w.put("a/x", &1u32).unwrap();
    w.commit().unwrap();

    // First read populates the path cache for this generation.
    let r1 = table.read().unwrap();
    assert_eq!(r1.get::<u32>("a/x").unwrap(), Some(1));

    // A second write changes the value and bumps the generation.
    let w = table.write().unwrap();
    w.put("a/x", &2u32).unwrap();
    w.commit().unwrap();

    // A fresh reader must see the new value, not a stale cached resolution.
    let r2 = table.read().unwrap();
    assert_eq!(r2.get::<u32>("a/x").unwrap(), Some(2));

    // The older snapshot still resolves to its own value (cache keyed by generation).
    assert_eq!(r1.get::<u32>("a/x").unwrap(), Some(1));
}

#[test]
fn map_roundtrips_and_is_accessible() {
    let (_dir, table) = table();

    let mut ages = BTreeMap::new();
    ages.insert("alice".to_string(), 30i32);
    ages.insert("bob".to_string(), 41i32);

    let w = table.write().unwrap();
    w.store("ages", &ages).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.load::<BTreeMap<String, i32>>("ages").unwrap(), ages);

    let map = r.fetch::<Map<i32>>("ages").unwrap();
    assert_eq!(map.len().unwrap(), 2);
    assert_eq!(map.keys().unwrap(), vec!["alice".to_string(), "bob".to_string()]);
    assert_eq!(map.get("bob").unwrap().unwrap().get().unwrap(), 41);
    assert!(map.get("carol").unwrap().is_none());
    assert!(map.contains_key("alice").unwrap());
    assert!(!map.contains_key("carol").unwrap());

    // Homogeneity: each value is a node reachable by raw path.
    assert_eq!(r.get::<i32>("ages/alice").unwrap(), Some(30));
}

#[test]
fn empty_map_materializes_an_object_node() {
    let (_dir, table) = table();

    let w = table.write().unwrap();
    w.store("empty", &BTreeMap::<String, i32>::new()).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.load::<BTreeMap<String, i32>>("empty").unwrap(), BTreeMap::new());
    assert_eq!(r.kind("empty").unwrap(), Some(NodeKind::Object));

    // The accessor works even though the map is empty.
    let map = r.fetch::<Map<i32>>("empty").unwrap();
    assert!(map.is_empty().unwrap());
}

#[test]
fn map_mut_insert_and_remove() {
    let (_dir, table) = table();

    let w = table.write().unwrap();
    let mut initial = BTreeMap::new();
    initial.insert("a".to_string(), 1i32);
    w.store("m", &initial).unwrap();
    {
        let m = w.fetch_mut::<MapMut<i32>>("m").unwrap();
        m.insert("b", &2).unwrap(); // {a: 1, b: 2}
        m.insert("a", &10).unwrap(); // replace -> {a: 10, b: 2}
        assert!(m.remove("a").unwrap()); // {b: 2}
        assert!(!m.remove("absent").unwrap());
        assert_eq!(m.get("b").unwrap().unwrap().get().unwrap(), 2);
    }
    w.commit().unwrap();

    let r = table.read().unwrap();
    let mut expected = BTreeMap::new();
    expected.insert("b".to_string(), 2i32);
    assert_eq!(r.load::<BTreeMap<String, i32>>("m").unwrap(), expected);
}
