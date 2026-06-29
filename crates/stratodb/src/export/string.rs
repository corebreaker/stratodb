//! Shared double-quoted string rendering. JSON strings and YAML double-quoted
//! scalars share an escaping scheme — the JSON escapes are a valid subset of
//! YAML's — so both writers quote through here.

/// Appends `s` to `out` as a double-quoted, escaped string.
pub(super) fn quote(out: &mut String, s: &str) {
    out.push('"');

    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }

    out.push('"');
}
