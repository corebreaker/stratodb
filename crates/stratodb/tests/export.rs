//! Integration tests for JSON and YAML export of a read transaction's subtrees.

use stratodb::{
    data::Scalar,
    error::SdbError,
    export::{JsonExporter, YamlExporter},
    path::SPath,
    StratoDb,
    Value,
};

fn mem_db() -> StratoDb {
    StratoDb::create_in_memory().expect("create db")
}

fn node(pairs: Vec<(&str, Value)>) -> Value {
    Value::Node(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

#[test]
fn empty_table_root_exports_as_null() {
    let db = mem_db();
    let table = db.open_table("data").unwrap();
    let r = table.read().unwrap();

    // The root of an empty table is `null`, addressed either as a string or an SPath.
    assert_eq!(r.export_to_json("", None).unwrap(), "null");
    assert_eq!(r.export_to_json(SPath::root(), Some(2)).unwrap(), "null");
    assert_eq!(r.export_to_yaml("").unwrap(), "null\n");

    // A non-root path on an empty table is still "not found".
    assert!(matches!(
        r.export_to_json("missing", None).unwrap_err(),
        SdbError::PathNotFound(_),
    ));
}

#[test]
fn exports_the_whole_table_from_the_root() {
    let db = mem_db();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.put("user/name", &String::from("Alice")).unwrap();
    w.put("user/age", &30u32).unwrap();
    w.put("user/active", &true).unwrap();
    w.put("user/score", &1.5f64).unwrap();
    w.put("user/nickname", &Option::<String>::None).unwrap();
    w.put_scalar("user/avatar", Scalar::Bytes(vec![0, 1, 2, 255])).unwrap();
    w.put("tags[0]", &String::from("x")).unwrap();
    w.put("tags[1]", &String::from("y")).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();

    let compact = concat!(
        r#"{"tags":["x","y"],"#,
        r#""user":{"active":true,"age":30,"avatar":"AAEC/w==","#,
        r#""name":"Alice","nickname":null,"score":1.5}}"#,
    );

    assert_eq!(r.export_to_json("", None).unwrap(), compact);

    let pretty = [
        "{",
        "  \"tags\": [",
        "    \"x\",",
        "    \"y\"",
        "  ],",
        "  \"user\": {",
        "    \"active\": true,",
        "    \"age\": 30,",
        "    \"avatar\": \"AAEC/w==\",",
        "    \"name\": \"Alice\",",
        "    \"nickname\": null,",
        "    \"score\": 1.5",
        "  }",
        "}",
    ]
    .join("\n");

    assert_eq!(r.export_to_json("", Some(2)).unwrap(), pretty);

    let yaml = [
        "\"tags\":",
        "  - \"x\"",
        "  - \"y\"",
        "\"user\":",
        "  \"active\": true",
        "  \"age\": 30",
        "  \"avatar\": \"AAEC/w==\"",
        "  \"name\": \"Alice\"",
        "  \"nickname\": null",
        "  \"score\": 1.5",
        "",
    ]
    .join("\n");

    assert_eq!(r.export_to_yaml("").unwrap(), yaml);
}

#[test]
fn exports_a_subtree_and_a_leaf() {
    let db = mem_db();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.put("user/name", &String::from("Alice")).unwrap();
    w.put("user/age", &30u32).unwrap();
    w.put("tags[0]", &String::from("x")).unwrap();
    w.put("tags[1]", &String::from("y")).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();

    // An object subtree.
    assert_eq!(r.export_to_json("user", None).unwrap(), r#"{"age":30,"name":"Alice"}"#);
    assert_eq!(r.export_to_yaml("user").unwrap(), "\"age\": 30\n\"name\": \"Alice\"\n");

    // A list subtree.
    assert_eq!(r.export_to_json("tags", None).unwrap(), r#"["x","y"]"#);

    // A single leaf, scalar and string.
    assert_eq!(r.export_to_json("user/age", None).unwrap(), "30");
    assert_eq!(r.export_to_json("user/name", None).unwrap(), "\"Alice\"");
    assert_eq!(r.export_to_yaml("user/age").unwrap(), "30\n");
}

#[test]
fn a_missing_path_is_not_found() {
    let db = mem_db();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.put("user/age", &30u32).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();

    assert!(matches!(
        r.export_to_json("user/ghost", None).unwrap_err(),
        SdbError::PathNotFound(_),
    ));

    assert!(matches!(
        r.export_to_yaml("ghost").unwrap_err(),
        SdbError::PathNotFound(_),
    ));
}

#[test]
fn exports_a_scalar_root() {
    let db = mem_db();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.put("", &String::from("hello")).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.export_to_json("", None).unwrap(), "\"hello\"");
    assert_eq!(r.export_to_yaml("").unwrap(), "\"hello\"\n");
}

#[test]
fn exports_a_list_root() {
    let db = mem_db();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.put("[0]", &1u32).unwrap();
    w.put("[1]", &2u32).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.export_to_json("", Some(2)).unwrap(), "[\n  1,\n  2\n]");
    assert_eq!(r.export_to_yaml("").unwrap(), "- 1\n- 2\n");
}

#[test]
fn a_stored_value_exports_consistently() {
    let db = mem_db();
    let table = db.open_table("data").unwrap();

    let value = node(vec![
        ("active", Value::Leaf(Scalar::Bool(true))),
        ("tags", Value::List(vec![Value::Leaf(Scalar::Str("x".into()))])),
    ]);

    let w = table.write().unwrap();
    w.store_value("", &value).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.export_to_json("", None).unwrap(), r#"{"active":true,"tags":["x"]}"#);
}

#[test]
fn an_in_memory_value_exports_itself_and_its_subtrees() {
    let value = node(vec![
        (
            "user",
            node(vec![
                ("name", Value::Leaf(Scalar::Str("Alice".into()))),
                ("age", Value::Leaf(Scalar::U32(30))),
            ]),
        ),
        ("tags", Value::List(vec![Value::Leaf(Scalar::Str("x".into()))])),
    ]);

    // The whole value at the root.
    assert_eq!(
        value.export_to_json("", None).unwrap(),
        r#"{"tags":["x"],"user":{"age":30,"name":"Alice"}}"#,
    );

    // A navigated subtree, an object then a single leaf.
    assert_eq!(
        value.export_to_json("user", None).unwrap(),
        r#"{"age":30,"name":"Alice"}"#
    );

    assert_eq!(value.export_to_yaml("user/name").unwrap(), "\"Alice\"\n");

    // A segment that leads nowhere.
    assert!(matches!(
        value.export_to_json("user/ghost", None).unwrap_err(),
        SdbError::PathNotFound(_),
    ));
}
