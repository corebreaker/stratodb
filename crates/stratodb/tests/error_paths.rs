//! Error and edge paths in the write pipeline: the kind guards and range checks
//! the tree enforces, plus the packed-entity overwrite path of a bulk store.

use stratodb::{data::SeqMut, error::SdbError, path::SPath, StratoDb, Table};

use std::collections::BTreeMap;

fn table() -> Table {
    StratoDb::create_in_memory()
        .expect("create db")
        .open_table("data")
        .expect("open table")
}

#[test]
fn writing_through_a_leaf_is_rejected() {
    let table = table();

    let w = table.write().unwrap();
    w.put("x", &1i32).unwrap();

    // "x" is a leaf, so materializing a container there (to hold "x/y") must fail.
    assert!(matches!(w.put("x/y", &2i32), Err(SdbError::UnexpectedNode { .. })));
}

#[test]
fn a_list_index_past_the_end_is_rejected() {
    let table = table();

    let w = table.write().unwrap();
    w.put("nums[0]", &10i32).unwrap();

    // Index 5 into a one-element list is out of range.
    assert!(matches!(
        w.put("nums[5]", &20i32),
        Err(SdbError::IndexOutOfRange {
            index: 5,
            len: 1,
            ..
        })
    ));
}

#[test]
fn swapping_out_of_range_list_elements_is_rejected() {
    let table = table();

    let w = table.write().unwrap();
    w.store("xs", &vec![1i32, 2, 3]).unwrap();
    {
        let xs = w.fetch_mut::<SeqMut<i32>>("xs").unwrap();

        assert!(matches!(
            xs.swap(0, 99),
            Err(SdbError::IndexOutOfRange {
                len: 3,
                ..
            })
        ));
    }
    w.commit().unwrap();
}

#[test]
fn indexing_into_a_non_list_resolves_to_absent() {
    let table = table();

    let w = table.write().unwrap();
    w.put("obj/field", &1i32).unwrap();
    w.commit().unwrap();

    // Indexing into an object (not a list) leads nowhere — absent, not an error.
    let r = table.read().unwrap();
    assert!(r.get::<i32>("obj[0]").unwrap().is_none());
    assert!(r.kind("obj[0]").unwrap().is_none());
}

#[test]
fn a_bulk_store_overwrites_existing_packed_entities_in_place() {
    let table = table();

    let (a1, b1) = (
        BTreeMap::from([(String::from("v"), 1i32)]),
        BTreeMap::from([(String::from("v"), 2i32)]),
    );
    let first = vec![
        (SPath::parse("items/a").unwrap(), &a1),
        (SPath::parse("items/b").unwrap(), &b1),
    ];

    let w = table.write().unwrap();
    w.store_many(&first).unwrap();
    w.commit().unwrap();

    // A second bulk store over the same (already packed) entities rewrites each
    // blob at its existing key.
    let (a2, b2) = (
        BTreeMap::from([(String::from("v"), 10i32)]),
        BTreeMap::from([(String::from("v"), 20i32)]),
    );
    let second = vec![
        (SPath::parse("items/a").unwrap(), &a2),
        (SPath::parse("items/b").unwrap(), &b2),
    ];

    let w = table.write().unwrap();
    w.store_many(&second).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.get::<i32>("items/a/v").unwrap(), Some(10));
    assert_eq!(r.get::<i32>("items/b/v").unwrap(), Some(20));
}
