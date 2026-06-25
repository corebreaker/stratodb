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

#[derive(SData, Debug, PartialEq)]
#[strato(rename_all = "camelCase")]
struct Renamed {
    first_name: String,
    #[strato(rename = "years")]
    age:        u32,
    #[strato(alias = "handle", alias = "nick")]
    nickname:   String,
}

#[test]
fn rename_rename_all_and_alias() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("renamed.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    let value = Renamed {
        first_name: String::from("Ada"),
        age:        36,
        nickname:   String::from("countess"),
    };

    let w = table.write().unwrap();
    w.store("p", &value).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();

    // Stored under the camelCase / renamed node names, not the Rust field names.
    assert_eq!(r.get::<String>("p/firstName").unwrap(), Some(String::from("Ada")));
    assert_eq!(r.get::<u32>("p/years").unwrap(), Some(36)); // field `rename` beats `rename_all`
    assert_eq!(r.get::<String>("p/nickname").unwrap(), Some(String::from("countess")));
    assert!(r.get::<String>("p/first_name").unwrap().is_none());
    assert!(r.get::<u32>("p/age").unwrap().is_none());

    // Full roundtrip; accessor method names are the Rust fields, nodes are renamed.
    assert_eq!(r.load::<Renamed>("p").unwrap(), value);

    let acc = r.fetch::<StratoRenamed>("p").unwrap();
    assert_eq!(acc.first_name().unwrap().get().unwrap(), "Ada");
    assert_eq!(acc.age().unwrap().get().unwrap(), 36);

    // The descriptor reports the stored names.
    assert_eq!(StratoRenamedDesc::FIELDS, &["firstName", "years", "nickname"]);
}

#[test]
fn alias_is_accepted_on_load() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("alias.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    // `nickname` data stored under one of its aliases (a legacy node name).
    let w = table.write().unwrap();
    w.put("p/firstName", &String::from("Bob")).unwrap();
    w.put("p/years", &40u32).unwrap();
    w.put("p/handle", &String::from("bobby")).unwrap();
    w.commit().unwrap();

    assert_eq!(
        table.read().unwrap().load::<Renamed>("p").unwrap(),
        Renamed {
            first_name: String::from("Bob"),
            age:        40,
            nickname:   String::from("bobby"),
        }
    );
}

fn default_level() -> u8 {
    7
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

#[derive(SData, Debug, PartialEq)]
struct Settings {
    name:          String,
    #[strato(skip)]
    runtime_cache: u64,
    #[strato(skip_store, default)]
    derived:       String,
    #[strato(skip_load)]
    version:       u32,
    #[strato(default)]
    retries:       u32,
    #[strato(default = "default_level")]
    level:         u8,
    #[strato(skip_store_if = "is_zero", default)]
    flags:         u32,
}

#[test]
fn skip_and_default_family_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("settings.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    let original = Settings {
        name:          String::from("db"),
        runtime_cache: 999,                       // skip -> never stored
        derived:       String::from("ephemeral"), // skip_store -> never stored
        version:       3,                         // skip_load -> stored, ignored on load
        retries:       5,
        level:         9,
        flags:         0b101, // != 0 -> stored
    };

    let w = table.write().unwrap();
    w.store("s", &original).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();

    // Never-stored fields leave no node; skip_load still stores.
    assert!(!r.exists("s/runtime_cache").unwrap());
    assert!(!r.exists("s/derived").unwrap());
    assert_eq!(r.get::<u32>("s/version").unwrap(), Some(3));
    assert_eq!(r.get::<u32>("s/flags").unwrap(), Some(0b101));

    // The accessor exists for skip_load (it is stored) but not for skipped fields.
    assert_eq!(
        r.fetch::<StratoSettings>("s")
            .unwrap()
            .version()
            .unwrap()
            .get()
            .unwrap(),
        3
    );

    // Load drops skip / skip_load values back to their defaults.
    assert_eq!(
        r.load::<Settings>("s").unwrap(),
        Settings {
            name:          String::from("db"),
            runtime_cache: 0,
            derived:       String::new(),
            version:       0,
            retries:       5,
            level:         9,
            flags:         0b101,
        }
    );

    // The descriptor omits never-stored fields.
    assert_eq!(
        StratoSettingsDesc::FIELDS,
        &["name", "version", "retries", "level", "flags"]
    );
}

#[test]
fn defaults_fill_absent_nodes() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("settings_partial.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    // Only `name` is present; everything else is absent.
    let w = table.write().unwrap();
    w.put("s/name", &String::from("db")).unwrap();
    w.commit().unwrap();

    assert_eq!(
        table.read().unwrap().load::<Settings>("s").unwrap(),
        Settings {
            name:          String::from("db"),
            runtime_cache: 0,             // skip
            derived:       String::new(), // skip_store + default
            version:       0,             // skip_load
            retries:       0,             // default -> Default
            level:         7,             // default = "default_level"
            flags:         0,             // default
        }
    );
}

#[test]
fn skip_store_if_omits_when_predicate_holds() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("settings_skipif.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.store(
        "s",
        &Settings {
            name:          String::from("x"),
            runtime_cache: 0,
            derived:       String::new(),
            version:       0,
            retries:       0,
            level:         0,
            flags:         0, // is_zero -> not stored
        },
    )
    .unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert!(!r.exists("s/flags").unwrap());
    assert_eq!(r.load::<Settings>("s").unwrap().flags, 0); // refilled by default
}

mod tenths {
    use stratodb::{
        access::{Reader, Writer},
        data::SData,
        path::SPath,
        SdbResult,
    };

    /// Stores an `f64` as its number of tenths (an `i64`) — a representation the
    /// field's own `SData` impl would never produce.
    pub fn store<W: Writer>(value: &f64, writer: &W, at: &SPath) -> SdbResult<()> {
        let tenths = (*value * 10.0).round() as i64;
        tenths.store(writer, at)
    }

    /// Recomposes the `f64` from the stored tenths.
    pub fn load<R: Reader>(reader: &R, at: &SPath) -> SdbResult<f64> {
        Ok(i64::load(reader, at)? as f64 / 10.0)
    }
}

#[derive(SData, Debug, PartialEq)]
struct Measurement {
    label:  String,
    #[strato(with = "tenths")]
    amount: f64,
}

#[test]
fn with_uses_the_custom_store_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("with.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    let m = Measurement {
        label:  String::from("len"),
        amount: 2.5,
    };

    let w = table.write().unwrap();
    w.store("m", &m).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();

    // The custom representation is visible by raw path: stored as i64 tenths,
    // which the field's plain `f64` `SData` impl would never produce.
    assert_eq!(r.get::<i64>("m/amount").unwrap(), Some(25));

    // Full roundtrip recomposes through the custom load.
    assert_eq!(r.load::<Measurement>("m").unwrap(), m);
}

#[derive(SData, Debug, PartialEq)]
struct Reading {
    #[strato(store_with = "tenths::store", load_with = "tenths::load", default)]
    value: f64,
}

#[test]
fn store_with_and_load_with_compose_with_default() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("reading.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.store(
        "r",
        &Reading {
            value: 1.5
        },
    )
    .unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();

    // Separate `store_with` / `load_with` use the same custom tenths shape.
    assert_eq!(r.get::<i64>("r/value").unwrap(), Some(15));
    assert_eq!(
        r.load::<Reading>("r").unwrap(),
        Reading {
            value: 1.5
        }
    );

    // An absent node falls back to `default`; the custom load is never reached.
    assert_eq!(
        r.load::<Reading>("absent").unwrap(),
        Reading {
            value: 0.0
        }
    );
}

#[derive(SData, Debug, PartialEq, Clone)]
#[strato(into = "String", try_from = "String")]
struct Email(String);

impl From<Email> for String {
    fn from(email: Email) -> String {
        email.0
    }
}

impl TryFrom<String> for Email {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.contains('@') {
            Ok(Email(value))
        } else {
            Err(format!("not an email address: {value}"))
        }
    }
}

#[test]
fn into_and_try_from_store_a_newtype_as_its_inner_type() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("email.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.store("e", &Email(String::from("ada@example.com"))).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();

    // Stored as a plain String leaf (the `into` target), not as a struct node.
    assert_eq!(r.get::<String>("e").unwrap(), Some(String::from("ada@example.com")));

    // Load reconstructs through `TryFrom`.
    assert_eq!(r.load::<Email>("e").unwrap(), Email(String::from("ada@example.com")));
}

#[test]
fn try_from_rejects_an_invalid_stored_value() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("email_bad.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    // A bare String that is not a valid Email, written directly.
    let w = table.write().unwrap();
    w.store("e", &String::from("not-an-email")).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();

    // The failed `TryFrom` surfaces as `SdbError::Conversion`.
    let error = r.load::<Email>("e").unwrap_err();
    assert!(matches!(error, stratodb::SdbError::Conversion(_)));
}

#[derive(SData, Debug, PartialEq, Clone)]
#[strato(into = "Vec<i64>", from = "Vec<i64>")]
struct Point {
    x: i64,
    y: i64,
}

impl From<Point> for Vec<i64> {
    fn from(point: Point) -> Vec<i64> {
        vec![point.x, point.y]
    }
}

impl From<Vec<i64>> for Point {
    fn from(values: Vec<i64>) -> Point {
        Point {
            x: values[0],
            y: values[1],
        }
    }
}

#[test]
fn into_and_from_store_a_struct_under_a_different_shape() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("point.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.store(
        "p",
        &Point {
            x: 3, y: 7
        },
    )
    .unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();

    // Stored as a `Vec<i64>` list, not as an object with `x`/`y` fields.
    assert_eq!(r.get::<i64>("p[0]").unwrap(), Some(3));
    assert_eq!(r.get::<i64>("p[1]").unwrap(), Some(7));
    assert!(!r.exists("p/x").unwrap());

    assert_eq!(
        r.load::<Point>("p").unwrap(),
        Point {
            x: 3, y: 7
        }
    );
}

#[derive(SData, Debug, PartialEq)]
struct Account {
    email: Email,
}

#[test]
fn delegated_field_exposes_the_target_types_accessor() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("account.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.store(
        "acc",
        &Account {
            email: Email(String::from("grace@example.com")),
        },
    )
    .unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();

    // `Email`'s accessor IS `String`'s: the getter yields a `Leaf<String>`.
    let acc = r.fetch::<StratoAccount>("acc").unwrap();
    assert_eq!(acc.email().unwrap().get().unwrap(), "grace@example.com");

    assert_eq!(
        r.load::<Account>("acc").unwrap(),
        Account {
            email: Email(String::from("grace@example.com")),
        }
    );
}

#[derive(SData, Debug, PartialEq)]
#[strato(rename_all = "snake_case")]
enum Event {
    Created,
    #[strato(rename = "deleted_at")]
    Deleted(i64),
    #[strato(alias = "modified")]
    Updated {
        version: u32,
    },
}

#[test]
fn enum_rename_all_variant_rename_and_alias() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("events.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.store("a", &Event::Created).unwrap();
    w.store("b", &Event::Deleted(5)).unwrap();
    w.store(
        "c",
        &Event::Updated {
            version: 2
        },
    )
    .unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();

    // Stored under the cased / renamed tags, not the Rust variant names.
    assert_eq!(r.fetch::<StratoEvent>("a").unwrap().variant().unwrap(), "created");
    assert_eq!(r.get::<i64>("b/deleted_at").unwrap(), Some(5));
    assert!(r.get::<i64>("b/Deleted").unwrap().is_none());
    assert_eq!(r.get::<u32>("c/updated/version").unwrap(), Some(2));

    // Roundtrips.
    assert_eq!(r.load::<Event>("a").unwrap(), Event::Created);
    assert_eq!(r.load::<Event>("b").unwrap(), Event::Deleted(5));
    assert_eq!(
        r.load::<Event>("c").unwrap(),
        Event::Updated {
            version: 2
        }
    );

    // The descriptor reports the stored tags.
    assert_eq!(StratoEventDesc::VARIANTS, &["created", "deleted_at", "updated"]);
}

#[test]
fn enum_variant_alias_is_accepted_on_load() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("events_alias.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    // Data stored under the alias tag "modified" (a legacy variant name).
    let w = table.write().unwrap();
    w.put("e/modified/version", &3u32).unwrap();
    w.commit().unwrap();

    assert_eq!(
        table.read().unwrap().load::<Event>("e").unwrap(),
        Event::Updated {
            version: 3
        }
    );
}

#[derive(SData, Debug, PartialEq)]
#[strato(tag = "kind", content = "payload")]
enum Cmd {
    Stop,
    Echo(String),
    Sum(i64, i64),
    Spawn { name: String, count: u32 },
}

#[test]
fn adjacently_tagged_enum_lays_out_tag_and_content_and_roundtrips() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("cmd.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.store("stop", &Cmd::Stop).unwrap();
    w.store("echo", &Cmd::Echo(String::from("hi"))).unwrap();
    w.store("sum", &Cmd::Sum(2, 3)).unwrap();
    w.store(
        "spawn",
        &Cmd::Spawn {
            name:  String::from("w"),
            count: 4,
        },
    )
    .unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();

    // Tag in the `kind` field, payload in `payload`; a unit variant has no content.
    assert_eq!(r.get::<String>("stop/kind").unwrap(), Some(String::from("Stop")));
    assert!(!r.exists("stop/payload").unwrap());

    assert_eq!(r.get::<String>("echo/kind").unwrap(), Some(String::from("Echo")));
    assert_eq!(r.get::<String>("echo/payload").unwrap(), Some(String::from("hi")));

    assert_eq!(r.get::<String>("sum/kind").unwrap(), Some(String::from("Sum")));
    assert_eq!(r.get::<i64>("sum/payload[0]").unwrap(), Some(2));
    assert_eq!(r.get::<i64>("sum/payload[1]").unwrap(), Some(3));

    assert_eq!(r.get::<String>("spawn/kind").unwrap(), Some(String::from("Spawn")));
    assert_eq!(r.get::<u32>("spawn/payload/count").unwrap(), Some(4));

    // Roundtrips through every variant shape.
    assert_eq!(r.load::<Cmd>("stop").unwrap(), Cmd::Stop);
    assert_eq!(r.load::<Cmd>("echo").unwrap(), Cmd::Echo(String::from("hi")));
    assert_eq!(r.load::<Cmd>("sum").unwrap(), Cmd::Sum(2, 3));
    assert_eq!(
        r.load::<Cmd>("spawn").unwrap(),
        Cmd::Spawn {
            name:  String::from("w"),
            count: 4,
        }
    );

    // The accessor reads the tag from the `kind` field, not the object key.
    assert_eq!(r.fetch::<StratoCmd>("echo").unwrap().variant().unwrap(), "Echo");
}

#[derive(SData, Debug, PartialEq)]
#[strato(tag = "type")]
enum Node {
    Leaf,
    Wrap(String),
    Pair(i64, i64),
    Branch { left: u32, right: u32 },
}

#[test]
fn internally_tagged_enum_flattens_tag_and_payload() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("node.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.store("leaf", &Node::Leaf).unwrap();
    w.store("wrap", &Node::Wrap(String::from("x"))).unwrap();
    w.store("pair", &Node::Pair(1, 2)).unwrap();
    w.store(
        "branch",
        &Node::Branch {
            left: 3, right: 4
        },
    )
    .unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();

    // Tag in `type`; payload flattened into the same object — tuple/newtype
    // elements keyed by their decimal index, struct fields by name.
    assert_eq!(r.get::<String>("leaf/type").unwrap(), Some(String::from("Leaf")));

    assert_eq!(r.get::<String>("wrap/type").unwrap(), Some(String::from("Wrap")));
    assert_eq!(r.get::<String>("wrap/0").unwrap(), Some(String::from("x")));

    assert_eq!(r.get::<String>("pair/type").unwrap(), Some(String::from("Pair")));
    assert_eq!(r.get::<i64>("pair/0").unwrap(), Some(1));
    assert_eq!(r.get::<i64>("pair/1").unwrap(), Some(2));

    assert_eq!(r.get::<String>("branch/type").unwrap(), Some(String::from("Branch")));
    assert_eq!(r.get::<u32>("branch/left").unwrap(), Some(3));
    assert_eq!(r.get::<u32>("branch/right").unwrap(), Some(4));

    // Roundtrips through every variant shape.
    assert_eq!(r.load::<Node>("leaf").unwrap(), Node::Leaf);
    assert_eq!(r.load::<Node>("wrap").unwrap(), Node::Wrap(String::from("x")));
    assert_eq!(r.load::<Node>("pair").unwrap(), Node::Pair(1, 2));
    assert_eq!(
        r.load::<Node>("branch").unwrap(),
        Node::Branch {
            left: 3, right: 4
        }
    );

    // The accessor reads the tag from the `type` field.
    assert_eq!(r.fetch::<StratoNode>("pair").unwrap().variant().unwrap(), "Pair");
}

#[derive(SData, Debug, PartialEq)]
#[strato(untagged)]
enum Value {
    Empty,
    Int(i64),
    Pair(i64, i64),
    Record { id: u32, name: String },
}

#[test]
fn untagged_enum_stores_bare_and_loads_by_trial() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("value.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.store("empty", &Value::Empty).unwrap();
    w.store("int", &Value::Int(42)).unwrap();
    w.store("pair", &Value::Pair(1, 2)).unwrap();
    w.store(
        "rec",
        &Value::Record {
            id:   7,
            name: String::from("x"),
        },
    )
    .unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();

    // Stored bare — no tag node; the payload sits directly at the path.
    assert_eq!(r.get::<i64>("int").unwrap(), Some(42));
    assert_eq!(r.get::<i64>("pair[0]").unwrap(), Some(1));
    assert_eq!(r.get::<i64>("pair[1]").unwrap(), Some(2));
    assert_eq!(r.get::<u32>("rec/id").unwrap(), Some(7));
    assert_eq!(r.get::<String>("rec/name").unwrap(), Some(String::from("x")));

    // Load picks the first variant whose shape fits, in declaration order.
    assert_eq!(r.load::<Value>("empty").unwrap(), Value::Empty);
    assert_eq!(r.load::<Value>("int").unwrap(), Value::Int(42));
    assert_eq!(r.load::<Value>("pair").unwrap(), Value::Pair(1, 2));
    assert_eq!(
        r.load::<Value>("rec").unwrap(),
        Value::Record {
            id:   7,
            name: String::from("x"),
        }
    );

    // An untagged enum stores no tag, so the accessor's variant() is unavailable.
    assert!(r.fetch::<StratoValue>("int").unwrap().variant().is_err());
}

#[derive(SData, Debug, PartialEq)]
#[strato(tag = "kind")]
enum Level {
    Low,
    #[strato(other)]
    Unknown,
}

#[test]
fn other_variant_catches_unknown_tags() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("level.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.store("low", &Level::Low).unwrap();
    w.store("unknown", &Level::Unknown).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();

    // A known tag and the `other` variant both roundtrip.
    assert_eq!(r.load::<Level>("low").unwrap(), Level::Low);
    assert_eq!(r.load::<Level>("unknown").unwrap(), Level::Unknown);

    // A tag matching no known variant loads as the `other` variant.
    let w = table.write().unwrap();
    w.put("weird/kind", &String::from("Sideways")).unwrap();
    w.commit().unwrap();

    assert_eq!(table.read().unwrap().load::<Level>("weird").unwrap(), Level::Unknown);
}

#[derive(SData, Debug)]
#[strato(tag = "kind", expecting = "a known level")]
enum Strict {
    On,
    Off,
}

#[test]
fn expecting_customizes_the_no_match_error() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("strict.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    // An unknown tag, with no `other` variant to absorb it.
    let w = table.write().unwrap();
    w.put("x/kind", &String::from("Maybe")).unwrap();
    w.commit().unwrap();

    let error = table.read().unwrap().load::<Strict>("x").unwrap_err();
    assert!(matches!(error, stratodb::SdbError::Corrupt(message) if message == "a known level"));
}

#[derive(SData, Debug, PartialEq)]
struct Wrapper<T> {
    value: T,
    label: String,
}

#[test]
fn generic_struct_roundtrips_across_instantiations() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("wrapper.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.store(
        "int",
        &Wrapper {
            value: 42i64,
            label: String::from("answer"),
        },
    )
    .unwrap();
    w.store(
        "text",
        &Wrapper {
            value: String::from("hi"),
            label: String::from("greeting"),
        },
    )
    .unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();

    // Shredded per field, for each instantiation of T.
    assert_eq!(r.get::<i64>("int/value").unwrap(), Some(42));
    assert_eq!(r.get::<String>("int/label").unwrap(), Some(String::from("answer")));

    assert_eq!(
        r.load::<Wrapper<i64>>("int").unwrap(),
        Wrapper {
            value: 42,
            label: String::from("answer"),
        }
    );
    assert_eq!(
        r.load::<Wrapper<String>>("text").unwrap(),
        Wrapper {
            value: String::from("hi"),
            label: String::from("greeting"),
        }
    );

    // The generated accessor is generic too.
    let acc = r.fetch::<StratoWrapper<'_, i64>>("int").unwrap();
    assert_eq!(acc.value().unwrap().get().unwrap(), 42);
}

#[derive(SData, Debug, PartialEq)]
enum Either<L, R> {
    Left(L),
    Right(R),
}

#[test]
fn generic_enum_roundtrips() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("either.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.store("l", &Either::<i64, String>::Left(7)).unwrap();
    w.store("r", &Either::<i64, String>::Right(String::from("x"))).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();

    assert_eq!(r.load::<Either<i64, String>>("l").unwrap(), Either::Left(7));
    assert_eq!(
        r.load::<Either<i64, String>>("r").unwrap(),
        Either::Right(String::from("x"))
    );
}

#[derive(Debug, PartialEq)]
struct NotSData;

#[derive(SData, Debug, PartialEq)]
#[strato(bound = "")]
struct Phantom<T> {
    name: String,
    #[strato(skip)]
    tag:  std::marker::PhantomData<T>,
}

#[test]
fn bound_overrides_the_default_sdata_bound() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("phantom.stratodb")).unwrap();
    let table = db.open_table("data").unwrap();

    // `NotSData` is not `SData`; the empty `bound` drops the default `T: SData`,
    // so `Phantom<NotSData>` derives at all.
    let value: Phantom<NotSData> = Phantom {
        name: String::from("x"),
        tag:  std::marker::PhantomData,
    };

    let w = table.write().unwrap();
    w.store("p", &value).unwrap();
    w.commit().unwrap();

    assert_eq!(
        table.read().unwrap().load::<Phantom<NotSData>>("p").unwrap(),
        Phantom {
            name: String::from("x"),
            tag:  std::marker::PhantomData,
        }
    );
}
