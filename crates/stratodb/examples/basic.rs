//! Basic StratoDB usage: open a database, then write and read values addressed
//! by path. No derive macro here — just the untyped path API.
//!
//! Run with: `cargo run -p stratodb --example basic`

use stratodb::{NodeKind, SdbResult, StratoDb};

fn main() -> SdbResult<()> {
    // A throwaway database file, cleaned up when `dir` is dropped.
    let dir = tempfile::tempdir().expect("create a temp dir");
    let db = StratoDb::create(dir.path().join("basic.stratodb"))?;
    let config = db.open_table("config")?;

    // Writes are transactional: stage changes, then commit.
    let w = config.write()?;
    w.put("server/host", &String::from("localhost"))?;
    w.put("server/port", &8080u32)?;
    w.put("server/tls", &true)?;
    w.commit()?;

    // Reads see committed data, decoded into the requested Rust type.
    let r = config.read()?;
    let host: Option<String> = r.get("server/host")?;
    let port: Option<u32> = r.get("server/port")?;
    let tls: Option<bool> = r.get("server/tls")?;
    println!("server = {}:{} (tls: {})", host.unwrap(), port.unwrap(), tls.unwrap());

    // Paths address a tree of nodes; `server` itself is an object node.
    assert_eq!(r.kind("server")?, Some(NodeKind::Object));

    Ok(())
}
