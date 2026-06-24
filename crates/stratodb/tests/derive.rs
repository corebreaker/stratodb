//! Validates `#[derive(SData)]`: the generated accessors and `SData` impl must
//! behave exactly like the hand-written reference in `typed.rs`.

use stratodb::{data::refs::SIdentifiable, SData, Skey, StratoDb};
use std::collections::BTreeMap;

#[derive(SData, Debug, PartialEq)]
struct Inner {
    y: i64,
}

#[derive(SData, Debug, PartialEq)]
struct Sample {
    x:     u32,
    inner: Inner,
}

#[test]
fn derived_struct_store_fetch_load_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("derive.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    let sample = Sample {
        x:     7,
        inner: Inner {
            y: -3
        },
    };

    let w = table.write().unwrap();
    w.store("a/h", &sample).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();

    // Generated read accessor: eager key, uniform `field()` getters.
    let acc = r.fetch::<StratoSample>("a/h").unwrap();
    let _pk: Skey = acc.key();
    assert_eq!(acc.x().unwrap().get().unwrap(), 7);
    assert_eq!(acc.inner().unwrap().y().unwrap().get().unwrap(), -3);

    // Full recomposition through the generated `SData::load`.
    let loaded: Sample = r.load("a/h").unwrap();
    assert_eq!(loaded, sample);

    // Homogeneity: the shredded leaves are also reachable by raw path.
    assert_eq!(r.get::<u32>("a/h/x").unwrap(), Some(7));
    assert_eq!(r.get::<i64>("a/h/inner/y").unwrap(), Some(-3));
}

#[test]
fn derived_struct_mutation_through_generated_setters() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("derive_mut.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.store(
        "a/h",
        &Sample {
            x:     1,
            inner: Inner {
                y: 2
            },
        },
    )
    .unwrap();
    {
        // Generated write accessor: uniform `field_mut()` getters returning the
        // mutable leaf / nested accessor.
        let acc = w.fetch_mut::<StratoSampleMut>("a/h").unwrap();
        let _pk: Skey = acc.key();
        acc.x_mut().unwrap().set(&42u32).unwrap();
        acc.inner_mut().unwrap().y_mut().unwrap().set(&99i64).unwrap();
    }
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(
        r.load::<Sample>("a/h").unwrap(),
        Sample {
            x:     42,
            inner: Inner {
                y: 99
            },
        }
    );
}

#[derive(SData, Debug, PartialEq)]
struct Profile {
    name:     String,
    tags:     Vec<String>,
    nickname: Option<String>,
    scores:   BTreeMap<String, i32>,
}

#[test]
fn derived_struct_with_container_fields() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("derive_containers.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    let mut scores = BTreeMap::new();
    scores.insert("art".to_string(), 75);
    scores.insert("math".to_string(), 90);

    let profile = Profile {
        name: "alice".to_string(),
        tags: vec!["a".to_string(), "b".to_string()],
        nickname: Some("al".to_string()),
        scores,
    };

    let w = table.write().unwrap();
    w.store("p", &profile).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();

    // Full recompose through the derived `SData::load`.
    assert_eq!(r.load::<Profile>("p").unwrap(), profile);

    // Each container field surfaces as its accessor via the uniform getter:
    // `Vec` -> Seq, `Option` -> OptRef, `BTreeMap` -> Map, scalar -> Leaf.
    let acc = r.fetch::<StratoProfile>("p").unwrap();
    assert_eq!(acc.name().unwrap().get().unwrap(), "alice");
    assert_eq!(acc.tags().unwrap().len().unwrap(), 2);
    assert_eq!(acc.tags().unwrap().get(1).unwrap().get().unwrap(), "b");
    assert_eq!(acc.nickname().unwrap().get().unwrap().unwrap().get().unwrap(), "al");
    assert_eq!(acc.scores().unwrap().get("math").unwrap().unwrap().get().unwrap(), 90);
    assert_eq!(
        acc.scores().unwrap().keys().unwrap(),
        vec!["art".to_string(), "math".to_string()]
    );
}
