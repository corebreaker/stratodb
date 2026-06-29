//! Cross-feature integration: a single derived entity travels through the typed
//! API, secondary indexes, the dynamic `Value`, and JSON/YAML export together —
//! the seams the per-feature suites each exercise in isolation.
//!
//! Gated on `derive` (the entities here use `#[derive(SData)]`); the big-number
//! module additionally needs the `bignum` family. Run the whole thing with
//! `cargo test --all-features`.
#![cfg(feature = "derive")]

use stratodb::{
    data::Scalar,
    export::{JsonExporter, YamlExporter},
    SData,
    StratoDb,
    Table,
    Value,
};

/// A renamed struct (camelCase keys) carrying an enum field and two indexes —
/// every seam at once.
#[derive(SData, Debug, PartialEq)]
#[strato(rename_all = "camelCase")]
#[strato(index(name = "by_team", columns(team)))]
#[strato(index(name = "by_email", columns(email), unique))]
struct Member {
    full_name: String,
    team:      String,
    email:     String,
    role:      Role,
}

/// An adjacently-tagged enum: a unit variant and a struct variant.
#[derive(SData, Debug, PartialEq)]
#[strato(tag = "kind", content = "data")]
enum Role {
    Member,
    Lead { reports: u32 },
}

fn seed(table: &Table) {
    let w = table.write().unwrap();
    w.store(
        "members/alice",
        &Member {
            full_name: String::from("Alice Eng"),
            team:      String::from("eng"),
            email:     String::from("alice@example.io"),
            role:      Role::Lead {
                reports: 3
            },
        },
    )
    .unwrap();
    w.store(
        "members/bob",
        &Member {
            full_name: String::from("Bob Eng"),
            team:      String::from("eng"),
            email:     String::from("bob@example.io"),
            role:      Role::Member,
        },
    )
    .unwrap();
    w.store(
        "members/carol",
        &Member {
            full_name: String::from("Carol Sales"),
            team:      String::from("sales"),
            email:     String::from("carol@example.io"),
            role:      Role::Member,
        },
    )
    .unwrap();
    w.commit().unwrap();
}

#[test]
fn derived_entity_indexes_queries_and_exports() {
    let db = StratoDb::create_in_memory().unwrap();
    let members = db.open_table("members").unwrap();

    // Both indexes that `Member` declares, scoped to `members/*`, back-filled.
    members.create_indexes::<Member>("members/*").unwrap();
    seed(&members);

    let r = members.read().unwrap();

    // The non-unique `by_team` index recomposes every matching entity as a `Member`.
    let mut eng: Vec<Member> = r.find("by_team", &[Scalar::Str(String::from("eng"))]).unwrap();
    eng.sort_by(|a, b| a.full_name.cmp(&b.full_name));
    assert_eq!(eng.len(), 2);
    assert_eq!(eng[0].full_name, "Alice Eng");
    assert_eq!(eng[1].full_name, "Bob Eng");

    // The unique `by_email` index rejects a second entity with an existing email.
    let w = members.write().unwrap();
    let duplicate = w.store(
        "members/dave",
        &Member {
            full_name: String::from("Dave"),
            team:      String::from("eng"),
            email:     String::from("alice@example.io"),
            role:      Role::Member,
        },
    );
    assert!(duplicate.is_err());
    w.abort();

    // Export reflects the renamed field (`fullName`) and the adjacently-tagged
    // enum (`{kind, data}`); object keys come out sorted.
    let json = r.export_to_json("members/alice", None).unwrap();
    assert_eq!(
        json,
        concat!(
            r#"{"email":"alice@example.io","fullName":"Alice Eng","#,
            r#""role":{"data":{"reports":3},"kind":"Lead"},"team":"eng"}"#,
        ),
    );

    // A unit variant exports as just its tag, with no `data` key.
    let bob = r.export_to_yaml("members/bob/role").unwrap();
    assert_eq!(bob, "\"kind\": \"Member\"\n");
}

#[test]
fn derived_entity_round_trips_through_a_dynamic_value() {
    let db = StratoDb::create_in_memory().unwrap();
    let members = db.open_table("members").unwrap();
    seed(&members);

    let r = members.read().unwrap();

    // The stored subtree loads into a faithful `Value` mirror, addressable by path.
    let value = r.load_value("members/alice").unwrap().unwrap();
    assert!(matches!(value, Value::Node(_)));
    assert_eq!(
        value.get_value("fullName"),
        Some(Value::Leaf(Scalar::Str("Alice Eng".into())))
    );
    assert_eq!(
        value.get_value("role/kind"),
        Some(Value::Leaf(Scalar::Str("Lead".into())))
    );
    assert_eq!(value.get_value("role/data/reports"), Some(Value::Leaf(Scalar::U32(3))));

    // Stored back through the dynamic path, it recomposes into the same typed
    // entity — the `Value` bridge is lossless for what the type needs.
    let w = members.write().unwrap();
    w.store_value("clones/alice", &value).unwrap();
    w.commit().unwrap();

    let r = members.read().unwrap();
    assert_eq!(
        r.load::<Member>("clones/alice").unwrap(),
        Member {
            full_name: String::from("Alice Eng"),
            team:      String::from("eng"),
            email:     String::from("alice@example.io"),
            role:      Role::Lead {
                reports: 3
            },
        },
    );
}

/// Big-number scalars flowing through a derived entity, its index, and export.
/// Needs the `bignum` family on top of `derive` (so `--all-features`).
#[cfg(feature = "bignum")]
mod bignum {
    use super::*;
    use num_bigint::BigInt;
    use num_rational::BigRational;
    use std::collections::BTreeMap;

    #[derive(SData, Debug, PartialEq)]
    #[strato(index(name = "by_balance", columns(balance)))]
    struct Account {
        owner:   String,
        balance: BigInt,
    }

    #[test]
    fn bigint_index_orders_by_value_across_byte_lengths_and_sign() {
        let db = StratoDb::create_in_memory().unwrap();
        let accounts = db.open_table("accounts").unwrap();
        accounts.create_indexes::<Account>("accounts/*").unwrap();

        // Insertion order is scrambled. The index must return ascending by value,
        // including across the one-byte/two-byte magnitude boundary (127 vs 128)
        // and across the sign — exactly where a fixed-width encoding misorders.
        let balances = [
            BigInt::from(128),
            BigInt::from(-1_000_000),
            BigInt::from(0),
            BigInt::from(127),
            BigInt::from(-1),
            BigInt::from(1_000_000),
        ];

        let w = accounts.write().unwrap();
        for (i, balance) in balances.iter().enumerate() {
            w.store(
                format!("accounts/a{i}"),
                &Account {
                    owner:   format!("owner{i}"),
                    balance: balance.clone(),
                },
            )
            .unwrap();
        }
        w.commit().unwrap();

        // An empty prefix matches every indexed entity, in index (ascending) order.
        let r = accounts.read().unwrap();
        let ordered: Vec<Account> = r.find("by_balance", &[]).unwrap();
        let got: Vec<BigInt> = ordered.into_iter().map(|account| account.balance).collect();

        let mut want = balances.to_vec();
        want.sort();
        assert_eq!(got, want);
    }

    #[test]
    fn bignum_scalars_export_through_a_read_transaction() {
        let db = StratoDb::create_in_memory().unwrap();
        let table = db.open_table("nums").unwrap();

        let mut node = BTreeMap::new();
        node.insert(
            "count".to_string(),
            Value::Leaf(Scalar::BigInt(BigInt::from(123_456_789))),
        );
        node.insert(
            "ratio".to_string(),
            Value::Leaf(Scalar::Rational(BigRational::new(BigInt::from(3), BigInt::from(4)))),
        );

        let w = table.write().unwrap();
        w.store_value("n", &Value::Node(node)).unwrap();
        w.commit().unwrap();

        // A BigInt renders verbatim; a rational as a quoted `num/den`.
        let r = table.read().unwrap();
        assert_eq!(
            r.export_to_json("n", None).unwrap(),
            r#"{"count":123456789,"ratio":"3/4"}"#
        );
    }
}
