//! Container `SData` impls: `Vec`, `Option`, and the packed `Bytes` newtype,
//! exercised through both the value API (`store`/`load`) and the accessors.

use stratodb::{
    data::{Bytes, Map, MapMut, OptMut, OptRef, Seq, SeqMut, refs::SIdentifiable},
    NodeKind,
    SdbError,
    StratoDb,
    Table,
};

use std::collections::BTreeMap;

fn table() -> Table {
    let db = StratoDb::create_in_memory().expect("create db");

    db.open_table("data").expect("open table")
}

#[test]
fn vec_roundtrips_and_is_accessible() {
    let table = table();

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
    let table = table();

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
    let table = table();

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
    let table = table();

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
    let table = table();

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
    let table = table();

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
    let table = table();

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
    let table = table();

    let mut ages = BTreeMap::new();
    ages.insert(String::from("alice"), 30i32);
    ages.insert(String::from("bob"), 41i32);

    let w = table.write().unwrap();
    w.store("ages", &ages).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.load::<BTreeMap<String, i32>>("ages").unwrap(), ages);

    let map = r.fetch::<Map<i32>>("ages").unwrap();
    assert_eq!(map.len().unwrap(), 2);
    assert_eq!(map.keys().unwrap(), vec![String::from("alice"), String::from("bob")]);
    assert_eq!(map.get("bob").unwrap().unwrap().get().unwrap(), 41);
    assert!(map.get("carol").unwrap().is_none());
    assert!(map.contains_key("alice").unwrap());
    assert!(!map.contains_key("carol").unwrap());

    // Homogeneity: each value is a node reachable by raw path.
    assert_eq!(r.get::<i32>("ages/alice").unwrap(), Some(30));
}

#[test]
fn empty_map_materializes_an_object_node() {
    let table = table();

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
    let table = table();

    let w = table.write().unwrap();
    let mut initial = BTreeMap::new();
    initial.insert(String::from("a"), 1i32);
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
    expected.insert(String::from("b"), 2i32);
    assert_eq!(r.load::<BTreeMap<String, i32>>("m").unwrap(), expected);
}

#[test]
fn seq_iteration_and_queries() {
    let table = table();

    let w = table.write().unwrap();
    w.store("xs", &vec![10i32, 20, 30]).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    let seq = r.fetch::<Seq<i32>>("xs").unwrap();

    let forward: Vec<i32> = seq.iter().unwrap().map(|item| item.unwrap().get().unwrap()).collect();
    assert_eq!(forward, vec![10, 20, 30]);

    // The adapters come free from the double-ended iterator.
    let reversed: Vec<i32> = seq
        .iter()
        .unwrap()
        .rev()
        .map(|item| item.unwrap().get().unwrap())
        .collect();
    assert_eq!(reversed, vec![30, 20, 10]);

    assert_eq!(seq.first().unwrap().unwrap().get().unwrap(), 10);
    assert_eq!(seq.last().unwrap().unwrap().get().unwrap(), 30);
    assert!(seq.contains(&20).unwrap());
    assert!(!seq.contains(&99).unwrap());
}

#[test]
fn seq_mut_reordering_and_bulk_ops() {
    let table = table();

    let w = table.write().unwrap();
    w.store("xs", &vec![1i32, 2, 3, 4, 5]).unwrap();
    {
        let xs = w.fetch_mut::<SeqMut<i32>>("xs").unwrap();

        xs.swap(0, 4).unwrap(); // [5, 2, 3, 4, 1]
        assert_eq!(xs.swap_remove(0).unwrap(), Some(5)); // [1, 2, 3, 4]
        assert_eq!(xs.pop_last().unwrap(), Some(4)); // [1, 2, 3]
        assert_eq!(xs.pop_first().unwrap(), Some(1)); // [2, 3]
        xs.extend([10, 11]).unwrap(); // [2, 3, 10, 11]
        xs.retain(|v| v % 2 == 1).unwrap(); // [3, 11]
        assert_eq!(xs.drain(0..1).unwrap(), vec![3]); // [11]
        assert_eq!(xs.first_mut().unwrap().unwrap().get().unwrap(), 11);
        assert_eq!(xs.last_mut().unwrap().unwrap().get().unwrap(), 11);
    }
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.load::<Vec<i32>>("xs").unwrap(), vec![11]);
}

#[test]
fn seq_clear_empties_the_list() {
    let table = table();

    let w = table.write().unwrap();
    w.store("xs", &vec![1i32, 2, 3]).unwrap();
    {
        let xs = w.fetch_mut::<SeqMut<i32>>("xs").unwrap();
        xs.clear().unwrap();
        assert!(xs.is_empty().unwrap());
    }
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.load::<Vec<i32>>("xs").unwrap(), Vec::<i32>::new());
    assert_eq!(r.kind("xs").unwrap(), Some(NodeKind::List));
}

fn sample_map() -> BTreeMap<String, i32> {
    let mut m = BTreeMap::new();
    m.insert(String::from("a"), 1);
    m.insert(String::from("b"), 2);
    m.insert(String::from("c"), 3);
    m
}

#[test]
fn map_iteration_and_queries() {
    let table = table();

    let w = table.write().unwrap();
    w.store("m", &sample_map()).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    let map = r.fetch::<Map<i32>>("m").unwrap();

    let pairs: Vec<(String, i32)> = map
        .iter()
        .unwrap()
        .map(|item| {
            let (key, value) = item.unwrap();
            (key, value.get().unwrap())
        })
        .collect();
    assert_eq!(
        pairs,
        vec![(String::from("a"), 1), (String::from("b"), 2), (String::from("c"), 3)]
    );

    let values: Vec<i32> = map.values().unwrap().map(|v| v.unwrap().get().unwrap()).collect();
    assert_eq!(values, vec![1, 2, 3]);

    // rev via the double-ended iterator.
    let rev_keys: Vec<String> = map.iter().unwrap().rev().map(|item| item.unwrap().0).collect();
    assert_eq!(rev_keys, vec![String::from("c"), String::from("b"), String::from("a")]);

    let (first_key, first_value) = map.first().unwrap().unwrap();
    assert_eq!((first_key.as_str(), first_value.get().unwrap()), ("a", 1));
    let (last_key, last_value) = map.last().unwrap().unwrap();
    assert_eq!((last_key.as_str(), last_value.get().unwrap()), ("c", 3));
}

#[test]
fn map_mut_bulk_ops() {
    let table = table();

    let w = table.write().unwrap();
    let mut initial = sample_map();
    initial.insert(String::from("d"), 4);
    w.store("m", &initial).unwrap();
    {
        let mm = w.fetch_mut::<MapMut<i32>>("m").unwrap();

        assert_eq!(mm.pop_first().unwrap(), Some((String::from("a"), 1))); // {b, c, d}
        assert_eq!(mm.pop_last().unwrap(), Some((String::from("d"), 4))); // {b, c}
        mm.extend([(String::from("x"), 10), (String::from("y"), 11)]).unwrap(); // {b, c, x, y}
        mm.retain(|key, _| key != "c").unwrap(); // {b, x, y}

        let (key, value) = mm.first_mut().unwrap().unwrap(); // smallest key = "b"
        assert_eq!(key, "b");
        value.set(&99).unwrap(); // b = 99
    }
    w.commit().unwrap();

    let r = table.read().unwrap();
    let mut expected = BTreeMap::new();
    expected.insert(String::from("b"), 99);
    expected.insert(String::from("x"), 10);
    expected.insert(String::from("y"), 11);
    assert_eq!(r.load::<BTreeMap<String, i32>>("m").unwrap(), expected);
}

#[test]
fn map_drain_and_clear() {
    let table = table();

    let w = table.write().unwrap();
    w.store("m", &sample_map()).unwrap();
    {
        let mm = w.fetch_mut::<MapMut<i32>>("m").unwrap();
        assert_eq!(mm.drain().unwrap(), sample_map());
        assert!(mm.is_empty().unwrap());
    }
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.load::<BTreeMap<String, i32>>("m").unwrap(), BTreeMap::new());
    assert_eq!(r.kind("m").unwrap(), Some(NodeKind::Object));
}

#[test]
fn loading_a_container_from_an_absent_path_is_empty_or_missing() {
    let table = table();

    // The table must exist (be written once) for an absent path to resolve to
    // `None` — the empty-container branch — rather than to a missing table.
    let w = table.write().unwrap();
    w.put("marker", &1u32).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.load::<Vec<i32>>("nope").unwrap(), Vec::<i32>::new());
    assert_eq!(r.load::<BTreeMap<String, i32>>("nope").unwrap(), BTreeMap::new());
    assert_eq!(r.load::<Option<i32>>("nope").unwrap(), None);
    assert!(matches!(r.load::<Bytes>("nope"), Err(SdbError::PathNotFound(_))));
}

#[test]
fn option_mut_and_accessor_identity() {
    let table = table();

    let w = table.write().unwrap();
    w.store("some", &Some(1i32)).unwrap();
    w.store("none", &Option::<i32>::None).unwrap();
    w.store("wrap", &Some(vec![1i32, 2, 3])).unwrap();
    {
        // A `Some` accessor reports present; `set(None)` then clears the node.
        // The accessor's key is eager, so it goes stale after `set` rewrites the
        // node — check the outcome through a fresh load, not the same accessor.
        let opt = w.fetch_mut::<OptMut<i32>>("some").unwrap();
        assert!(!opt.is_none().unwrap());
        assert!(!opt.path().is_empty());
        let _ = opt.key();
        opt.set(&None).unwrap();
    }
    {
        // A `None` accessor reports absent; `set(Some(..))` fills it.
        let opt = w.fetch_mut::<OptMut<i32>>("none").unwrap();
        assert!(opt.is_none().unwrap());
        opt.set(&Some(9)).unwrap();
    }
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.load::<Option<i32>>("some").unwrap(), None);
    assert_eq!(r.load::<Option<i32>>("none").unwrap(), Some(9));

    // A `Some(composite)` is a non-leaf node, so `is_none` takes the non-leaf path.
    let wrap = r.fetch::<OptRef<Vec<i32>>>("wrap").unwrap();
    assert!(!wrap.is_none().unwrap());
    let _ = wrap.key();
    assert_eq!(wrap.path().to_string(), "wrap");
    assert_eq!(wrap.get().unwrap().unwrap().len().unwrap(), 3);
}

#[test]
fn seq_accessor_edges() {
    let table = table();

    let w = table.write().unwrap();
    w.store("xs", &vec![1i32, 2, 3, 4]).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    let seq = r.fetch::<Seq<i32>>("xs").unwrap();
    let _ = seq.key();
    assert_eq!(seq.path().to_string(), "xs");

    // Each element accessor is a `Leaf`, carrying its own key and path.
    let elem = seq.get(0).unwrap();
    let _ = elem.key();
    assert_eq!(elem.path().to_string(), "xs[0]");
    assert_eq!(elem.get().unwrap(), 1);

    assert!(matches!(
        seq.get(99),
        Err(SdbError::IndexOutOfRange {
            index: 99,
            len: 4,
            ..
        })
    ));

    let w = table.write().unwrap();
    {
        let xs = w.fetch_mut::<SeqMut<i32>>("xs").unwrap();
        let _ = xs.key();
        assert_eq!(xs.path().to_string(), "xs");

        // A mutable element accessor is a `LeafMut`, likewise identifiable.
        let elem = xs.get(0).unwrap();
        let _ = elem.key();
        assert_eq!(elem.path().to_string(), "xs[0]");
        assert_eq!(elem.get().unwrap(), 1);
        assert!(matches!(
            xs.get(99),
            Err(SdbError::IndexOutOfRange {
                index: 99,
                len: 4,
                ..
            })
        ));
        assert!(xs.contains(&3).unwrap());
        assert!(!xs.contains(&99).unwrap());

        // A range past the end stops at the last element (the break arm).
        xs.remove_range(2..10).unwrap(); // [1, 2]
        assert_eq!(xs.len().unwrap(), 2);

        assert_eq!(xs.swap_remove(99).unwrap(), None);
    }
    w.commit().unwrap();

    // pop on an emptied list returns None.
    let w = table.write().unwrap();
    {
        let xs = w.fetch_mut::<SeqMut<i32>>("xs").unwrap();
        xs.clear().unwrap();
        assert_eq!(xs.pop_first().unwrap(), None);
        assert_eq!(xs.pop_last().unwrap(), None);
    }
    w.commit().unwrap();
}

#[test]
fn map_accessor_edges() {
    let table = table();

    let w = table.write().unwrap();
    w.store("m", &sample_map()).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    let map = r.fetch::<Map<i32>>("m").unwrap();
    let _ = map.key();
    assert_eq!(map.path().to_string(), "m");

    let w = table.write().unwrap();
    {
        let mm = w.fetch_mut::<MapMut<i32>>("m").unwrap();
        let _ = mm.key();
        assert_eq!(mm.path().to_string(), "m");
        assert!(mm.contains_key("a").unwrap());
        assert!(!mm.contains_key("zzz").unwrap());
        assert!(mm.get("zzz").unwrap().is_none());

        let values: Vec<i32> = mm.values_mut().unwrap().map(|v| v.unwrap().get().unwrap()).collect();
        assert_eq!(values, vec![1, 2, 3]);

        let (last_key, _) = mm.last_mut().unwrap().unwrap();
        assert_eq!(last_key, "c");
    }
    w.commit().unwrap();

    // pop on an emptied map returns None.
    let w = table.write().unwrap();
    {
        let mm = w.fetch_mut::<MapMut<i32>>("m").unwrap();
        mm.clear().unwrap();
        assert_eq!(mm.pop_first().unwrap(), None);
        assert_eq!(mm.pop_last().unwrap(), None);
    }
    w.commit().unwrap();
}
