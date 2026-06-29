//! Typed entities and secondary indexes, declared with `#[derive(SData)]` and
//! `#[strato(index(...))]`.
//!
//! Run with: `cargo run -p stratodb --example indexed --features derive`

use stratodb::{data::Scalar, SData, SdbResult, StratoDb};

#[derive(SData, Debug)]
#[strato(index(name = "by_team", columns(team)))]
#[strato(index(name = "by_email", columns(email), unique))]
struct Member {
    name:  String,
    team:  String,
    email: String,
}

fn main() -> SdbResult<()> {
    let dir = tempfile::tempdir().expect("create a temp dir");
    let db = StratoDb::create(dir.path().join("members.stratodb"))?;
    let members = db.open_table("members")?;

    // Register every index `Member` declares, scoped to the `members/*` entities.
    members.create_indexes::<Member>("members/*")?;

    let w = members.write()?;
    for (id, name, team, email) in [
        ("alice", "Alice", "eng", "alice@example.io"),
        ("bob", "Bob", "eng", "bob@example.io"),
        ("carol", "Carol", "sales", "carol@example.io"),
    ] {
        w.store(
            format!("members/{id}"),
            &Member {
                name:  name.to_string(),
                team:  team.to_string(),
                email: email.to_string(),
            },
        )?;
    }
    w.commit()?;

    // Query the `by_team` index; each hit recomposes into a `Member`.
    let r = members.read()?;
    let mut eng: Vec<Member> = r.find("by_team", &[Scalar::Str(String::from("eng"))])?;
    eng.sort_by(|a, b| a.name.cmp(&b.name));
    println!("eng team: {:?}", eng.iter().map(|m| &m.name).collect::<Vec<_>>());

    // `by_email` is unique, so a duplicate email is rejected (the write rolls back).
    let w = members.write()?;
    let duplicate = w.store(
        "members/dave",
        &Member {
            name:  String::from("Dave"),
            team:  String::from("eng"),
            email: String::from("alice@example.io"),
        },
    );
    println!("duplicate email rejected: {}", duplicate.is_err());

    Ok(())
}
