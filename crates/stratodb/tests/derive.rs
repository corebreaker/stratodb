//! Validates `#[derive(SData)]`: the generated accessors and `SData` impl must
//! behave exactly like the hand-written reference in `typed.rs`.
//!
//! The derive lives behind the `derive` feature, so this whole file compiles
//! away unless it is enabled (`cargo test --features derive`).
#![cfg(feature = "derive")]

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
    scores.insert(String::from("art"), 75);
    scores.insert(String::from("math"), 90);

    let profile = Profile {
        name: String::from("alice"),
        tags: vec![String::from("a"), String::from("b")],
        nickname: Some(String::from("al")),
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
        vec![String::from("art"), String::from("math")]
    );
}

#[derive(SData, Debug, PartialEq)]
enum Shape {
    Unit,
    Circle(f64),
    Rect(u32, u32),
    Labeled { name: String, size: i64 },
}

#[test]
fn derived_enum_roundtrips_every_variant_shape() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("derive_enum.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    let cases = [
        Shape::Unit,
        Shape::Circle(2.5),
        Shape::Rect(3, 4),
        Shape::Labeled {
            name: String::from("sq"),
            size: -1,
        },
    ];

    for (i, shape) in cases.iter().enumerate() {
        let path = format!("s{i}");

        let w = table.write().unwrap();
        w.store(&path, shape).unwrap();
        w.commit().unwrap();

        let r = table.read().unwrap();
        assert_eq!(&r.load::<Shape>(&path).unwrap(), shape);
    }
}

#[test]
fn derived_enum_is_externally_tagged_and_accessor_reports_variant() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("derive_enum_tag.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.store("circle", &Shape::Circle(1.5)).unwrap();
    w.store("rect", &Shape::Rect(3, 4)).unwrap();
    w.store(
        "sq",
        &Shape::Labeled {
            name: String::from("s"),
            size: 9,
        },
    )
    .unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();

    // The accessor reports the active variant tag.
    assert_eq!(r.fetch::<StratoShape>("circle").unwrap().variant().unwrap(), "Circle");
    assert_eq!(r.fetch::<StratoShape>("rect").unwrap().variant().unwrap(), "Rect");

    // External tagging is visible by raw path: newtype payload is stored
    // directly, a tuple payload as a list, a struct payload as an object.
    assert_eq!(r.get::<f64>("circle/Circle").unwrap(), Some(1.5));
    assert_eq!(r.get::<u32>("rect/Rect[0]").unwrap(), Some(3));
    assert_eq!(r.get::<u32>("rect/Rect[1]").unwrap(), Some(4));
    assert_eq!(r.get::<i64>("sq/Labeled/size").unwrap(), Some(9));
}

#[test]
fn derived_enum_restore_replaces_previous_variant() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("derive_enum_replace.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.store("s", &Shape::Rect(1, 2)).unwrap();
    w.commit().unwrap();

    // Re-storing a different variant must leave exactly one tag: the externally
    // tagged store clears the previous variant's subtree first.
    let w = table.write().unwrap();
    w.store("s", &Shape::Unit).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.load::<Shape>("s").unwrap(), Shape::Unit);
    assert_eq!(r.fetch::<StratoShape>("s").unwrap().variant().unwrap(), "Unit");
    assert_eq!(r.get::<u32>("s/Rect[0]").unwrap(), None); // the old payload is gone
}

#[test]
fn derived_descriptors_expose_members() {
    assert_eq!(StratoSampleDesc::TYPE_NAME, "Sample");
    assert_eq!(StratoSampleDesc::FIELDS.to_vec(), vec!["x", "inner"]);

    assert_eq!(StratoProfileDesc::TYPE_NAME, "Profile");
    assert_eq!(
        StratoProfileDesc::FIELDS.to_vec(),
        vec!["name", "tags", "nickname", "scores"]
    );

    assert_eq!(StratoShapeDesc::TYPE_NAME, "Shape");
    assert_eq!(
        StratoShapeDesc::VARIANTS.to_vec(),
        vec!["Unit", "Circle", "Rect", "Labeled"]
    );
}
