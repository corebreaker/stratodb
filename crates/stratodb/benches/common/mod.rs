//! Shared fixtures for the StratoDB benchmark suite.
//!
//! Every bench target (`reads`, `writes`, `modifications`, `deletes`, `indexes`,
//! `dynamic_value`) pulls its dataset, entity type and helpers from here, so the
//! numbers across categories all describe the same shape of data. Built behind the
//! `derive` feature (the benches use a `#[derive(SData)]` entity), so they run with
//! `cargo bench --features derive`.
//!
//! Each target uses a subset of these helpers, hence the crate-wide `dead_code`
//! allowance.
#![allow(dead_code)]

use stratodb::{SData, StratoDb, Table};

/// Entities in a read-oriented fixture. Big enough that the B-tree has realistic
/// depth; small enough that building it once per bench group stays fast (it is a
/// single transaction). Bump it for a heavier comparison run.
pub const DATASET: usize = 5_000;

/// Distinct keys cycled by the mutating benches (writes / modifications). Keeping
/// the working set bounded means the table never grows without limit across
/// criterion's many iterations, while still exercising the full code path.
pub const RING: usize = 1_000;

/// A flat record with one unique and one non-unique indexable column, plus a few
/// scalars of different types — representative of a typical stored entity.
#[derive(SData, Clone, Debug)]
#[strato(index(name = "by_age", columns(age)))]
#[strato(index(name = "by_email", columns(email), unique))]
pub struct User {
    pub name:   String,
    pub age:    u32,
    pub email:  String,
    pub score:  i64,
    pub active: bool,
}

impl User {
    /// A deterministic sample with a per-`i` unique email (so the unique index
    /// never collides) and an age bucketed into 100 values (so a `by_age` lookup
    /// returns many hits).
    pub fn sample(i: usize) -> User {
        User {
            name:   format!("user-{i}"),
            age:    (i % 100) as u32,
            email:  format!("user{i}@example.io"),
            score:  i as i64,
            active: i.is_multiple_of(2),
        }
    }
}

/// An in-memory database holding `count` users at `users/{i}`, written in a single
/// transaction. When `indexed`, the two secondary indexes are created first, so
/// every write also pays index maintenance.
pub fn populated(count: usize, indexed: bool) -> (StratoDb, Table) {
    let db = StratoDb::create_in_memory().expect("create in-memory db");
    let table = db.open_table("users").expect("open table");

    if indexed {
        table.create_indexes::<User>("users/*").expect("create indexes");
    }

    let w = table.write().expect("begin write");
    for i in 0..count {
        w.store(format!("users/{i}"), &User::sample(i)).expect("store user");
    }
    w.commit().expect("commit");

    (db, table)
}

/// A fixture for the mutating benches: a [`RING`]-sized table plus the pre-built
/// paths and entities a bench cycles through, so per-iteration work measures the
/// database operation rather than `format!`/`String` allocation.
pub struct Ring {
    pub db:    StratoDb,
    pub table: Table,
    pub paths: Vec<String>,
    pub users: Vec<User>,
}

impl Ring {
    pub fn new(indexed: bool) -> Ring {
        let (db, table) = populated(RING, indexed);
        let paths = (0..RING).map(|i| format!("users/{i}")).collect();
        let users = (0..RING).map(User::sample).collect();

        Ring {
            db,
            table,
            paths,
            users,
        }
    }
}
